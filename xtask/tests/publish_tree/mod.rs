//! A workspace that passes `cargo xtask publish`, with every floor met:
//! `baud` and nine crates it reaches, by a dependency, a workspace
//! dependency, a build-dependency and a target-specific one, the four
//! unpublished members, a sidecar with a row shipped in `c1`, and a
//! manifest of a hundred hyphenated source keys. Cargo runs offline, with
//! a home and a target folder of the tree's own beside it, so nothing
//! under the user's cargo home is read or written. A module directory, so
//! cargo does not build it as a test target of its own.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::publish::{self, Cargo, Exit, Flags, Pack, Release, Writes};

/// A directory under the system's temporary directory, removed on drop:
/// the workspace is `ws` inside it, beside cargo's home and target.
pub struct Tree {
    pub top: PathBuf,
    pub ws: PathBuf,
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.top);
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub const MIT: &str = "MIT License\n\nPermission is hereby granted.\n";
pub const LICENCE: &str = "baud's licence\n";
/// The crates `baud` reaches, beside itself.
pub const REACHED: [&str; 9] = ["c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9"];

pub fn flags() -> Flags {
    Flags {
        pack: Pack::No,
        notices: Writes::No,
        licences: Writes::No,
        release: Release::No,
    }
}

pub fn release() -> Flags {
    Flags {
        release: Release::Yes,
        ..flags()
    }
}

fn crate_manifest(name: &str, licence: &str, extra: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion.workspace = true\nedition.workspace = true\n\
         repository.workspace = true\nrust-version.workspace = true\nlicense = \"{licence}\"\n\
         description = \"The {name} crate.\"\nreadme = \"README.md\"\nkeywords = [\"terminal\"]\n\
         categories = [\"command-line-utilities\"]\n{extra}"
    )
}

const PROFILE: &str = "[profile.release]\nlto = \"fat\"\ncodegen-units = 1\n";

fn sidecar() -> String {
    format!(
        "[text.\"MIT.txt\"]\nlicence = \"MIT\"\nhow = \"fetched\"\nfrom = \"https://example.org/mit\"\n\
         sha256 = \"{}\"\n\n\
         [[tree]]\nwhat = \"A vendored table\"\npaths = [\"c1/vendored.txt\"]\nlicence = \"MIT\"\n\
         notice = \"LICENSES/MIT.txt\"\nupstream = \"https://example.org/vendored\"\nstatus = \"kept\"\n\
         public = \"A table copied from an upstream project.\"\nholder = \"The Example Authors\"\n\n\
         [upstream.doc-key-000]\nname = \"Doc\"\nlicence = \"MIT\"\nnotice = \"LICENSES/MIT.txt\"\n\
         status = \"kept\"\nfiles = [\"doc-000.txt\"]\n",
        xtask::sha256::hex(MIT.as_bytes())
    )
}

fn manifest() -> String {
    let mut m = String::new();
    for n in 0..100 {
        m.push_str(&format!(
            "[sources.doc-key-{n:03}]\nkey = \"doc-key-{n:03}\"\nfile = \"doc-{n:03}.txt\"\n\
             url = \"https://example.org/{n:03}\"\n\n"
        ));
    }
    m
}

impl Tree {
    /// A workspace of the given members' manifests, each with a lib.rs.
    pub fn bare() -> Tree {
        let top = std::env::temp_dir().join(format!(
            "xtask-publish-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&top);
        fs::create_dir_all(top.join("ws")).expect("the tree");
        let top = fs::canonicalize(&top).expect("the tree's real path");
        let ws = top.join("ws");
        Tree { top, ws }
    }

    /// The passing workspace, its NOTICEs written by the command.
    pub fn base() -> Tree {
        let t = Tree::bare();
        let unpub = ["baud-bench", "baud-difftest", "baud-sfte-difftest", "xtask"];
        let mut members: Vec<&str> = vec!["baud"];
        members.extend(REACHED);
        members.extend(unpub);
        let quoted: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
        t.put(
            "Cargo.toml",
            &format!(
                "[workspace]\nresolver = \"2\"\nmembers = [{}]\n\n[workspace.package]\n\
                 version = \"0.1.0\"\nedition = \"2021\"\nrepository = \"https://example.org/baud\"\n\
                 rust-version = \"1.80\"\n\n[workspace.dependencies]\n\
                 c9 = {{ path = \"c9\", version = \"0.1.0\" }}\n\n{PROFILE}",
                quoted.join(", ")
            ),
        );
        t.put("LICENSE", LICENCE);
        t.put("LICENSES/MIT.txt", MIT);
        t.put("docs/research/third-party.toml", &sidecar());
        t.put("docs/research/sources/MANIFEST.toml", &manifest());
        let dep = |n: &str| format!("{n} = {{ path = \"../{n}\", version = \"0.1.0\" }}\n");
        let mut deps = String::from("\n[dependencies]\n");
        for n in &REACHED[..6] {
            deps.push_str(&dep(n));
        }
        deps.push_str("c9 = { workspace = true }\n\n[build-dependencies]\n");
        deps.push_str(&dep("c7"));
        deps.push_str("\n[target.'cfg(target_os = \"linux\")'.dependencies]\n");
        deps.push_str(&dep("c8"));
        deps.push('\n');
        deps.push_str(PROFILE);
        t.put(
            "baud/Cargo.toml",
            &crate_manifest("baud", "CC-BY-NC-SA-4.0", &deps),
        );
        for n in std::iter::once("baud").chain(REACHED) {
            if n != "baud" {
                let licence = if n == "c1" {
                    "CC-BY-NC-SA-4.0 AND MIT"
                } else {
                    "CC-BY-NC-SA-4.0"
                };
                t.put(&format!("{n}/Cargo.toml"), &crate_manifest(n, licence, ""));
            }
            t.put(&format!("{n}/src/lib.rs"), "");
            t.put(&format!("{n}/README.md"), &format!("# {n}\n"));
            t.put(&format!("{n}/LICENSE"), LICENCE);
        }
        t.put("c1/vendored.txt", "a vendored table\n");
        t.put("c1/LICENSES/MIT.txt", MIT);
        for n in unpub {
            t.put(
                &format!("{n}/Cargo.toml"),
                &format!(
                    "[package]\nname = \"{n}\"\nversion.workspace = true\nedition.workspace = true\n\
                     publish = false\n"
                ),
            );
            t.put(&format!("{n}/src/lib.rs"), "");
        }
        let written = Flags {
            notices: Writes::Yes,
            ..flags()
        };
        let (exit, out) = t.run(written);
        assert_eq!(exit, Exit::Passed, "the base tree's NOTICEs:\n{out}");
        t
    }

    pub fn cargo(&self) -> Cargo {
        Cargo {
            home: self.top.join("cargo-home"),
            target: self.top.join("target"),
            env: vec![("CARGO_NET_OFFLINE".to_string(), "true".to_string())],
        }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.ws.join(rel)
    }

    pub fn put(&self, rel: &str, text: &str) {
        put(&self.path(rel), text);
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).expect(rel)
    }

    /// Replaces the one `from` in `rel`, which must be there.
    pub fn edit(&self, rel: &str, from: &str, to: &str) {
        let s = self.read(rel);
        assert_eq!(s.matches(from).count(), 1, "{rel} holds {from:?} once");
        self.put(rel, &s.replacen(from, to, 1));
    }

    pub fn run(&self, flags: Flags) -> (Exit, String) {
        let mut out = Vec::new();
        let exit = publish::run(&self.ws, flags, &self.cargo(), &mut out).expect("a run");
        (exit, String::from_utf8(out).expect("UTF-8"))
    }
}

pub fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("a parent")).expect("the folder");
    fs::write(path, text).expect("the file");
}
