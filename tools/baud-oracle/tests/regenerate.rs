//! **Every oracle regenerates its committed file byte for byte**, and a
//! changed input makes `--check` fail: the identity gate the port was made
//! against, kept as a test so a later engine, binding or format change has
//! to fail here.

use std::path::{Path, PathBuf};

use baud_oracle::oracle::ALL;
use baud_oracle::{Oracle, compare, repo};

fn committed(o: &Oracle) -> String {
    std::fs::read_to_string(repo().join(o.file)).expect("the committed file")
}

#[test]
fn every_oracle_regenerates_its_committed_file() {
    for o in &ALL {
        let lines = (o.lines)(&repo());
        let file = committed(o);
        compare(&lines, &file).unwrap_or_else(|e| panic!("{}: {e}", o.name));
        assert!(
            lines.join("\n") + "\n" == file,
            "{}: not byte for byte",
            o.name
        );
    }
}

/// A table's offset in a font, from its directory.
fn table(data: &[u8], tag: &[u8; 4]) -> usize {
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&o| &data[o..o + 4] == tag)
        .map(|o| u32::from_be_bytes(data[o + 8..o + 12].try_into().expect("4 bytes")) as usize)
        .unwrap_or_else(|| panic!("no {} table", String::from_utf8_lossy(tag)))
}

/// What a perturbation does to the copied tree.
enum Change {
    /// Put this test font in the place of that one.
    Swap(&'static str),
    /// Add 37 to the first `hmtx` advance.
    Advance,
    /// Make `post` version 3, which names no glyph.
    NoNames,
}

/// One perturbation per oracle, each on a font that oracle reads; the same
/// ones the Python and Rust arms were run on side by side.
const PERTURBATIONS: [(&str, &str, Change); 8] = [
    ("advance", "var/gvar-cases.ttf", Change::Advance),
    (
        "bearing",
        "var/bearing-cases-bit1.ttf",
        Change::Swap("fonts/JetBrainsMono-Variable.ttf"),
    ),
    (
        "cff2",
        "var/cff2-cases.otf",
        Change::Swap("fonts/JetBrainsMono-Variable.ttf"),
    ),
    (
        "component",
        "var/component-cases.ttf",
        Change::Swap("fonts/JetBrainsMono-Variable.ttf"),
    ),
    (
        "gvar",
        "var/gvar-cases.ttf",
        Change::Swap("fonts/SourceCodeVF-Upright.otf"),
    ),
    (
        "marks",
        "var/ink-cases.ttf",
        Change::Swap("fonts/FiraCode-VF.ttf"),
    ),
    (
        "var",
        "fonts/FiraCode-VF.ttf",
        Change::Swap("fonts/SourceCodeVF-Upright.otf"),
    ),
    ("vertical", "var/vertical-cases.ttf", Change::NoNames),
];

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a directory");
    for e in std::fs::read_dir(from).expect("a directory") {
        let e = e.expect("an entry");
        let dst = to.join(e.file_name());
        if e.file_type().expect("a type").is_dir() {
            copy_dir(&e.path(), &dst);
        } else {
            std::fs::copy(e.path(), dst).expect("a copy");
        }
    }
}

/// The test fonts copied under a fresh root, with `change` made to `font`.
fn perturbed(oracle: &str, font: &str, change: &Change) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "baud-oracle-perturbed-{}-{oracle}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let tests = root.join("baud-font/tests");
    for d in ["fonts", "var"] {
        copy_dir(&repo().join("baud-font/tests").join(d), &tests.join(d));
    }
    let path = tests.join(font);
    let mut data = std::fs::read(&path).expect("the font");
    match change {
        Change::Swap(other) => {
            data = std::fs::read(repo().join("baud-font/tests").join(other)).expect("a font");
        }
        Change::Advance => {
            let o = table(&data, b"hmtx");
            let w = u16::from_be_bytes([data[o], data[o + 1]]) + 37;
            data[o..o + 2].copy_from_slice(&w.to_be_bytes());
        }
        Change::NoNames => {
            let o = table(&data, b"post");
            data[o..o + 4].copy_from_slice(&[0, 3, 0, 0]);
        }
    }
    std::fs::write(&path, data).expect("write the font");
    root
}

/// **The positive control of every `--check`**: one changed input per
/// oracle, and the committed file no longer matches.
#[test]
fn a_changed_font_fails_every_check() {
    assert_eq!(
        PERTURBATIONS.len(),
        ALL.len(),
        "one perturbation per oracle"
    );
    for (name, font, change) in &PERTURBATIONS {
        let o = ALL.iter().find(|o| o.name == *name).expect("an oracle");
        let root = perturbed(name, font, change);
        let lines = (o.lines)(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            compare(&lines, &committed(o)).is_err(),
            "{name}: {font} changed and the check still passed"
        );
    }
}
