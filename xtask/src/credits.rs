//! `cargo xtask credits [--check|--crates]`: `baud-app/assets/CREDITS.txt`,
//! the notices of every third-party work the app's binary carries, written
//! from `docs/research/third-party.toml` and the licence texts its rows
//! point to, or checked against them byte for byte.
//!
//! **The crates.io crates the binary links are a notice of their own**,
//! written from the register `docs/research/crates.toml` ([`crates`]):
//! each crate with the platforms that link it, its copyright lines and the
//! licence it falls under, whose text joins the others printed once. The
//! check fails when the set `Cargo.lock` links differs from the register,
//! or a row's notice file in the registry no longer hashes to it;
//! `--crates` writes the register, and is the one mode that reads the
//! cargo registry.
//!
//! **What the binary carries is found, not listed.** Every
//! `include_bytes!` and `include_str!` in the shipped crates' sources (the
//! app and the path crates it depends on, read from their manifests) is
//! read, and the tables `baud-tablegen` writes as `include_bytes!` from a
//! build script. Each one outside `#[cfg(test)]` and `#[test]` must fall
//! under a `[[tree]]` row's `paths`, through the link the include names or
//! the file it reaches, or be one of baud's own files in [`OWN`], with its
//! reason. A new include with neither fails, because the include that is
//! forgotten is the one never listed. A file the build embeds only when it
//! is there, and which is not there, is [`PENDING`]: it fails once it lands
//! without a row.
//!
//! **Code a build writes is followed to what the build read.** An
//! `include!` of a file in `OUT_DIR` in a shipped crate compiles in what its
//! build script made, so every file that build reads ([`build_inputs`]:
//! the crate's `data/` and `protocols/`, and each file its build sources
//! name, as their `rerun-if-changed` lines do) must fall under a row or
//! [`OWN`] too.
//!
//! **Code transcribed into baud's sources is listed, not found**: each row
//! marked `transcribed = true` names the files it was ported from
//! (`ported_from`, each one named by the row's words, upstream or notice),
//! and its notice is in the file whatever the scan reaches.
//!
//! **Each row reached carries a `[tree.credit]` table**: a title, where baud
//! uses it, what was changed, its copyright lines, and its licence, and,
//! for the icon font, a `[[tree.credit.part]]` per icon set. Nothing in it
//! is taken on trust: every copyright line, creator and address must be in
//! the row's notice or a file the part names, word for word (whitespace
//! aside), and a font's copyright and trademark notices (name IDs 0 and
//! 7) are read out of the font itself. A row that carries a credit and is
//! not reached fails too.
//!
//! **The notices come first, then each licence text once**: the SIL OFL
//! 1.1, MIT and Apache 2.0 in full, and every other text a notice names,
//! from the files in the tree (each OFL and MIT notice is checked to hold
//! the same text as the one printed), with the permission notices a
//! work's own notice file gives (`[[tree.credit.text]]`, read out of the
//! file between two lines it holds); CC BY 4.0, CC BY-ND 4.0 and the
//! Unlicense by their URIs, as their terms allow. Two texts with the
//! same words, whitespace and markup aside, are one text, printed once with
//! the copyright lines of every work under it listed above it. Every notice
//! names the text it falls under.
//!
//! **A tripwire, not a limit**: the file failing past [`TRIPWIRE`] tells a
//! reviewer something large was added, and the failure prints the size and
//! the largest sections. Nothing is ever cut to fit.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use regex::Regex;
use toml::{Table, Value};

use crate::crates;
use crate::html::{At, Count};
use crate::third_party::{self, Outcome};

/// The file the command writes, and the app embeds.
pub const OUT: &str = "baud-app/assets/CREDITS.txt";
/// The crate the binary is, and where the dependency walk starts.
pub const APP: &str = "baud-app";
/// The file's first line.
pub const HEADER: &str = "Third-party notices for what this build of baud includes.";
/// The command the stale line tells a reader to run.
pub const GENERATOR: &str = "cargo xtask credits";

/// A size in bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Bytes(pub usize);

/// **The size past which the check fails**: 112 KiB, against the 97,396
/// bytes the file comes to once the data the builds compile in and the
/// code transcribed into baud's sources carry their notices (it was 64 KiB
/// against 39,986 bytes before). A tripwire: a file that needs more is
/// reviewed and this raised in a commit that says why; required text is
/// never cut to fit.
pub const TRIPWIRE: Bytes = Bytes(112 * 1024);

/// Whether an entry is reached by the shipped binary or only by tests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reach {
    Shipped,
    Test,
}

/// **One of baud's own files the binary embeds**, or a file baud makes from
/// third-party data: `from` names a path of each row whose material it
/// carries, so that row's notice is in the file.
pub struct Own {
    /// The include's path from the repository root; a generated file is
    /// `<crate>/OUT_DIR/<name>`.
    pub path: &'static str,
    /// A path each row it carries names.
    pub from: &'static [&'static str],
    pub why: &'static str,
}

/// baud's own embedded files, each with its reason.
pub const OWN: [Own; 7] = [
    Own {
        path: "baud-app/config.toml.example",
        from: &[],
        why: "the commented example config baud writes when it finds none: baud's own text",
    },
    Own {
        path: "baud-app/src/shell/zshenv.zsh",
        from: &[],
        why: "baud's own zsh integration, which it hands the shell it starts",
    },
    Own {
        path: "baud-app/src/shell/baud.bash",
        from: &[],
        why: "baud's own bash integration, which it hands the shell it starts",
    },
    Own {
        path: "baud-app/src/shell/baud.fish",
        from: &[],
        why: "baud's own fish integration, which it hands the shell it starts",
    },
    Own {
        path: "baud-app/assets/CREDITS.txt",
        from: &[],
        why: "this file: the notices themselves, generated from the rows it names",
    },
    Own {
        path: "baud-present-macos/OUT_DIR/shaders.metal",
        from: &["baud-present-macos/src/baud.metallib"],
        why: "baud's own shaders, with the stress mark's skip table spliced in, which \
              baud-render's build compiles from the Unicode Character Database: the text \
              baud.metallib is compiled from, whose row is the table's",
    },
    Own {
        path: "baud-domain/OUT_DIR/contrast.bin",
        from: &["baud-domain/src/contrast.rs"],
        why: "the text-contrast curve, a table baud-domain's build computes from \
              Windows Terminal's coefficients",
    },
];

/// **A file the build embeds only when it is in the tree**, and which is
/// not yet: it can be in no build made from this tree, so it needs no
/// notice yet. Once it is there it needs a row like any other include.
pub struct Pending {
    pub path: &'static str,
    /// Where its licence is recorded until its row lands.
    pub notice: &'static str,
}

/// The files the build embeds only once they are there, each with the
/// notice its row will name.
pub const PENDING: [Pending; 0] = [];

/// **The files the scan must find shipped**: its control, so a scan that
/// stopped finding includes, or read real ones as test code, fails rather
/// than passing on nothing.
pub const MUST_SHIP: [&str; 10] = [
    "baud-app/fonts/built/Regular.ttf",
    "baud-app/fonts/built/BoldItalic.ttf",
    "baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf",
    "baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf",
    "baud-app/assets/logo/baud-lockup-dark-tight.svg",
    "baud-app/assets/logo/baud-icon-dock.svg",
    "baud-app/terminfo/xterm-256color.ti",
    "baud-app/config.toml.example",
    "baud-present-macos/src/baud.metallib",
    "baud-app/assets/CREDITS.txt",
];

/// **And the includes it must find only in tests**: a guard reading its own
/// source (`take.rs`'s, which reads itself), and one reading a file a
/// third-party row names, so that test code is told apart rather than
/// everything being shipped or nothing.
pub const MUST_TEST: [&str; 2] = ["baud-app/src/take.rs", "baud-render/src/trail.rs"];

/// A `rerun-if-changed` a build script prints from a value it computes,
/// `{}` and an argument, which the scan cannot read as a path: what it
/// names, and why that is under a row, or baud's own, or no input of the
/// binary's.
pub struct Computed {
    /// The build source, from the repository root.
    pub file: &'static str,
    /// The argument printed, as the source spells it.
    pub arg: &'static str,
    pub why: &'static str,
}

/// **Every computed `rerun-if-changed` of a shipped crate's build**, so a
/// new one fails until it is read and listed here.
pub const COMPUTED: [Computed; 4] = [
    Computed {
        file: "baud-app/build.rs",
        arg: "path.display()",
        why: "the git files build_id::rerun_on names, read for the build's own identifier",
    },
    Computed {
        file: "baud-present-macos/build.rs",
        arg: "path.display()",
        why: "the stress tables in baud-render's OUT_DIR, which baud-render's build writes \
              from the data under its rows",
    },
    Computed {
        file: "baud-xkb/build.rs",
        arg: "path.display()",
        why: "xkbcommon-keysyms.h joined to the crate's folder, a build input the scan reads \
              by its name",
    },
    Computed {
        file: "baud-wayland/build.rs",
        arg: "path.display()",
        why: "each of FILES joined to protocols/, a folder the scan reads whole",
    },
];

/// One `rerun-if-changed` a build source prints: the source, what it
/// prints after the `=`, and the argument a `{}` in it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Print {
    pub file: String,
    pub printed: String,
    pub arg: Option<String>,
}

/// **What a crate's build says it reads**: each `rerun-if-changed=` its
/// `build.rs` and `build/` sources print, read from the sources, not from a
/// build's output, so the answer does not depend on what was built.
pub fn build_prints(root: &Path, krate: &str) -> Result<Vec<Print>, String> {
    let at =
        Regex::new(r#"rerun-if-changed=([^"]*)"(?:\s*,\s*([^;\n]*?)\)\s*;)?"#).expect("a print");
    let mut sources = rust_files(root, &format!("{krate}/build"));
    if root.join(krate).join("build.rs").is_file() {
        sources.push(format!("{krate}/build.rs"));
    }
    let mut out = Vec::new();
    for f in sources {
        let src = fs::read_to_string(root.join(&f)).map_err(|e| format!("{f}: {e}"))?;
        for line in src.lines().filter(|l| !l.trim_start().starts_with("//")) {
            for c in at.captures_iter(line) {
                out.push(Print {
                    file: f.clone(),
                    printed: c[1].to_owned(),
                    arg: c.get(2).map(|m| m.as_str().trim().to_owned()),
                });
            }
        }
    }
    Ok(out)
}

// ── The licence texts ────────────────────────────────────────────────────

/// Where a licence's text comes from.
pub enum Source {
    /// A file, from the first line holding `from` (or its start) to the
    /// line before the one holding `to` (or its end).
    File {
        path: &'static str,
        from: Option<&'static str>,
        to: Option<&'static str>,
    },
    /// The URI alone, as the licence allows, and a file of the tree that
    /// spells it so, which the check reads.
    Uri {
        uri: &'static str,
        seen: &'static str,
        /// Why the URI stands for the text.
        why: &'static str,
    },
}

/// A licence a notice can fall under.
pub struct Licence {
    /// The key a credit names: an SPDX identifier.
    pub key: &'static str,
    pub title: &'static str,
    pub source: Source,
    /// The words every notice of a work under it must hold, which says the
    /// notice is the licence's and not another's.
    pub marker: &'static str,
    /// For a Creative Commons licence, whose notice must refer to its
    /// disclaimer (3(a)(1)(A)(iv)): the words the notice refers to it by,
    /// and a file of the tree that spells them so.
    pub disclaimer: Option<(&'static str, &'static str)>,
}

/// Every licence a credit may name, in the order the texts are printed.
pub const LICENCES: [Licence; 9] = [
    Licence {
        key: "OFL-1.1",
        title: "SIL Open Font License, Version 1.1",
        source: Source::File {
            path: "baud-app/fonts/built/OFL.txt",
            from: Some("SIL OPEN FONT LICENSE Version 1.1"),
            to: None,
        },
        marker: "Open Font License",
        disclaimer: None,
    },
    Licence {
        key: "MIT",
        title: "MIT License",
        source: Source::File {
            path: "baud-app/fonts/symbols-nerd-font/LICENSE",
            from: Some("Permission is hereby granted"),
            to: None,
        },
        marker: "Permission is hereby granted",
        disclaimer: None,
    },
    Licence {
        key: "Apache-2.0",
        title: "Apache License, Version 2.0",
        source: Source::File {
            path: "LICENSES/Apache-2.0.txt",
            from: None,
            to: None,
        },
        marker: "Apache",
        disclaimer: None,
    },
    Licence {
        key: "X11",
        title: "X11 License, as ncurses gives it",
        source: Source::File {
            path: "LICENSES/ncurses-COPYING.txt",
            from: None,
            to: None,
        },
        marker: "Permission is hereby granted",
        disclaimer: None,
    },
    Licence {
        key: "Unicode-3.0",
        title: "Unicode License V3",
        source: Source::File {
            path: "LICENSES/Unicode-3.0.txt",
            from: None,
            to: None,
        },
        marker: "UNICODE LICENSE V3",
        disclaimer: None,
    },
    Licence {
        key: "Bitstream-Vera",
        title: "Bitstream Vera License",
        source: Source::File {
            path: "baud-app/fonts/symbols-nerd-font/icon-licences/extraglyphs/LICENSE.md",
            from: Some("Copyright (c) 2003 by Bitstream, Inc."),
            to: None,
        },
        marker: "Bitstream Vera",
        disclaimer: None,
    },
    Licence {
        key: "CC-BY-4.0",
        title: "Creative Commons Attribution 4.0 International",
        source: Source::Uri {
            uri: "https://creativecommons.org/licenses/by/4.0/",
            seen: "baud-app/fonts/symbols-nerd-font/icon-licences/font-awesome/LICENSE.txt",
            why: "Given by its URI, as its section 3(a)(1)(C) allows in place of its text:",
        },
        marker: "Attribution 4.0 International",
        disclaimer: Some((CC_BY_DISCLAIMER, CC_BY_LEGALCODE)),
    },
    Licence {
        key: "CC-BY-ND-4.0",
        title: "Creative Commons Attribution-NoDerivatives 4.0 International",
        source: Source::Uri {
            uri: "https://creativecommons.org/licenses/by-nd/4.0/",
            seen: CC_BY_ND_SEEN,
            why: "Given by its URI, as its section 3(a)(1)(C) allows in place of its text:",
        },
        marker: "Attribution-NoDerivatives 4.0 International",
        disclaimer: Some((CC_BY_ND_DISCLAIMER, CC_BY_ND_SEEN)),
    },
    Licence {
        key: "Unlicense",
        title: "The Unlicense",
        source: Source::Uri {
            uri: "www.unlicense.org",
            seen: "docs/research/sources/stb_truetype.h",
            why: "It asks for no notice; its text is at:",
        },
        marker: "",
        disclaimer: None,
    },
];

/// The file of the tree that gives CC BY-ND 4.0 by its URI and names its
/// disclaimer: the one work under it, which says so in its header.
pub const CC_BY_ND_SEEN: &str = "baud-font/data/adobe-5176-cff-standard-encoding.txt";

/// How a notice of a CC BY-ND work refers to the licence's disclaimer, as
/// that header names it.
pub const CC_BY_ND_DISCLAIMER: &str =
    "section 5, Disclaimer of Warranties and Limitation of Liability";

/// The heading of CC BY 4.0's section 5, which a notice of a CC BY work
/// refers to (3(a)(1)(A)(iv)), as the legal code in the tree spells it.
pub const CC_BY_DISCLAIMER: &str =
    "Section 5 -- Disclaimer of Warranties and Limitation of Liability";
/// The legal code that heading is read from.
pub const CC_BY_LEGALCODE: &str =
    "baud-app/fonts/symbols-nerd-font/icon-licences/codicons/LICENSE.txt";

/// The labels every CC BY work's notice carries, one per part of its
/// section 3(a)(1): the creator, the copyright notice, the notice referring
/// to the licence, the one referring to its disclaimer, the URI to the
/// material, the changes, and the licence's URI.
pub const CC_BY_PARTS: [&str; 7] = [
    "Creator:",
    "Copyright notice:",
    "Licence:",
    "Disclaimer:",
    "Material:",
    "Changes:",
    "Licence URI:",
];

/// The licence a credit names by `key`.
pub fn licence(key: &str) -> Option<&'static Licence> {
    LICENCES.iter().find(|l| l.key == key)
}

/// Letters for the texts' sections.
const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// The letter of the `n`th licence text: A to Z, then AA, AB and on.
pub fn letter(n: Count) -> String {
    let all: Vec<char> = LETTERS.chars().collect();
    let k = all.len();
    match n.0 < k {
        true => all[n.0].to_string(),
        false => format!("{}{}", all[(n.0 / k - 1) % k], all[n.0 % k]),
    }
}

// ── The scan ─────────────────────────────────────────────────────────────

/// What an include's argument names.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Names {
    /// A path from the repository root.
    Path(String),
    /// A file the build writes: `<crate>/OUT_DIR/<name>`.
    Built(String),
    /// What the scan cannot read, as written.
    Unread(String),
}

/// Which macro an include is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Macro {
    /// `include_bytes!` or `include_str!`: the file's bytes are in the binary.
    Data,
    /// `include!`: the file is Rust code, compiled in; for a file the build
    /// writes, what the build read to write it is in the binary.
    Code,
}

/// One `include_bytes!`, `include_str!` or `include!`.
#[derive(Clone, Debug)]
pub struct Include {
    /// The source file, from the repository root.
    pub file: String,
    pub line: Count,
    pub names: Names,
    pub reach: Reach,
    pub kind: Macro,
}

/// A token of Rust source: what the scan reads, comments and the insides of
/// character literals gone.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Tok {
    Ident(String),
    Str(String),
    Punct(char),
}

/// The source's tokens with their lines.
fn lex(src: &str) -> Vec<(Tok, Count)> {
    let b: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    let lines = |s: &[char]| s.iter().filter(|&&c| c == '\n').count();
    while i < b.len() {
        let c = b[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            let (mut depth, start) = (0usize, i);
            while i < b.len() {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            line += lines(&b[start..i.min(b.len())]);
        } else if c == '"'
            || ((c == 'r' || c == 'b' || c == 'c')
                && matches!(b.get(i + 1), Some('"') | Some('#') | Some('r'))
                && raw_or_prefixed(&b, At(i)))
        {
            let start = i;
            let (value, At(end)) = string(&b, At(i));
            out.push((Tok::Str(value), Count(line)));
            line += lines(&b[start..end]);
            i = end;
        } else if c == '\'' {
            // A character literal, or a lifetime: `'x'`, `'\n'`, `'\u{..}'`.
            if b.get(i + 1) == Some(&'\\') {
                let mut j = i + 2;
                while j < b.len() && b[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
            } else if b.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push((Tok::Ident(b[start..i].iter().collect()), Count(line)));
        } else {
            out.push((Tok::Punct(c), Count(line)));
            i += 1;
        }
    }
    out
}

/// Whether the `r`, `b` or `c` at `i` starts a string literal rather than
/// an identifier.
fn raw_or_prefixed(b: &[char], at: At) -> bool {
    let i = at.0;
    if i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_') {
        return false;
    }
    let mut j = i;
    if matches!(b.get(j), Some('b') | Some('c')) {
        j += 1;
    }
    if b.get(j) == Some(&'r') {
        j += 1;
        while b.get(j) == Some(&'#') {
            j += 1;
        }
    }
    b.get(j) == Some(&'"') && j > i
}

/// The string literal at `i`, its value with the simple escapes undone,
/// and the index after it.
fn string(b: &[char], at: At) -> (String, At) {
    let mut i = at.0;
    while b[i] == 'b' || b[i] == 'c' {
        i += 1;
    }
    let mut v = String::new();
    if b[i] == 'r' {
        i += 1;
        let mut hashes = 0;
        while b[i] == '#' {
            hashes += 1;
            i += 1;
        }
        i += 1;
        while i < b.len() {
            if b[i] == '"' && (0..hashes).all(|k| b.get(i + 1 + k) == Some(&'#')) {
                return (v, At(i + 1 + hashes));
            }
            v.push(b[i]);
            i += 1;
        }
        return (v, At(i));
    }
    i += 1;
    while i < b.len() {
        match b[i] {
            '"' => return (v, At(i + 1)),
            '\\' => {
                match b.get(i + 1) {
                    Some('n') => v.push('\n'),
                    Some('t') => v.push('\t'),
                    Some('\n') => {
                        i += 2;
                        while i < b.len() && b[i].is_whitespace() {
                            i += 1;
                        }
                        continue;
                    }
                    Some(&e) => v.push(e),
                    None => {}
                }
                i += 2;
            }
            ch => {
                v.push(ch);
                i += 1;
            }
        }
    }
    (v, At(i))
}

/// Whether an attribute's tokens, between its brackets, gate what follows
/// on `test` alone: `test`, `cfg(test)` or `cfg(all(test, ..))`.
fn gates_on_test(attr: &[Tok]) -> bool {
    let id = |t: &Tok, s: &str| matches!(t, Tok::Ident(x) if x == s);
    match attr {
        [t] => id(t, "test"),
        [cfg, Tok::Punct('('), t, Tok::Punct(')')] => id(cfg, "cfg") && id(t, "test"),
        [cfg, Tok::Punct('('), all, Tok::Punct('('), t, ..] => {
            id(cfg, "cfg") && id(all, "all") && id(t, "test")
        }
        _ => false,
    }
}

/// A `mod name;` a file declares, with whether it is test code and the
/// file a `#[path]` names.
struct Child {
    name: String,
    path: Option<String>,
    reach: Reach,
}

/// What one file holds: its includes, as written, and its child modules.
struct Read {
    includes: Vec<(Count, Vec<Tok>, Reach, Macro)>,
    children: Vec<Child>,
    /// The `macro_rules!` that expand to one string, by name.
    strings: BTreeMap<String, String>,
}

/// The includes and child modules of a file whose module is `reach`.
fn read_file(src: &str, reach: Reach) -> Read {
    let toks = lex(src);
    let mut out = Read {
        includes: Vec::new(),
        children: Vec::new(),
        strings: BTreeMap::new(),
    };
    // Each open brace: whether what is inside is test code.
    let mut braces: Vec<Reach> = Vec::new();
    let mut nest = 0usize; // parentheses and brackets
    // A test gate waiting for its item, at the nesting it was written at.
    let mut gate: Option<usize> = None;
    let mut path_attr: Option<String> = None;
    let mut whole = reach;
    let here = |braces: &[Reach], whole: Reach| {
        if whole == Reach::Test || braces.contains(&Reach::Test) {
            Reach::Test
        } else {
            Reach::Shipped
        }
    };
    let mut i = 0;
    while i < toks.len() {
        match &toks[i].0 {
            Tok::Punct('#') => {
                let inner = toks.get(i + 1).map(|t| &t.0) == Some(&Tok::Punct('!'));
                let open = if inner { i + 2 } else { i + 1 };
                if toks.get(open).map(|t| &t.0) == Some(&Tok::Punct('[')) {
                    let mut depth = 0;
                    let mut j = open;
                    while j < toks.len() {
                        match toks[j].0 {
                            Tok::Punct('[') => depth += 1,
                            Tok::Punct(']') => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    let attr: Vec<Tok> = toks[open + 1..j].iter().map(|t| t.0.clone()).collect();
                    if gates_on_test(&attr) {
                        if inner && braces.is_empty() {
                            whole = Reach::Test;
                        } else if !inner {
                            gate = Some(nest);
                        }
                    }
                    if let [Tok::Ident(p), Tok::Punct('='), Tok::Str(s)] = attr.as_slice() {
                        if p == "path" {
                            path_attr = Some(s.clone());
                        }
                    }
                    i = j + 1;
                    continue;
                }
            }
            Tok::Punct('{') => {
                let inside = match gate.take() {
                    Some(_) => Reach::Test,
                    None => here(&braces, whole),
                };
                braces.push(inside);
                path_attr = None;
            }
            Tok::Punct('}') => {
                braces.pop();
            }
            Tok::Punct('(') | Tok::Punct('[') => nest += 1,
            Tok::Punct(')') | Tok::Punct(']') => nest = nest.saturating_sub(1),
            Tok::Punct(';') => {
                if gate == Some(nest) {
                    gate = None;
                }
                path_attr = None;
            }
            Tok::Ident(m) if m == "mod" => {
                if let (Some((Tok::Ident(name), _)), Some((Tok::Punct(';'), _))) =
                    (toks.get(i + 1), toks.get(i + 2))
                {
                    let r = match gate {
                        Some(_) => Reach::Test,
                        None => here(&braces, whole),
                    };
                    out.children.push(Child {
                        name: name.clone(),
                        path: path_attr.take(),
                        reach: r,
                    });
                }
            }
            Tok::Ident(m) if m == "macro_rules" => {
                // `macro_rules! name { () => { "text" }; }` with one string.
                if let Some((Tok::Ident(name), _)) = toks.get(i + 2) {
                    let mut depth = 0;
                    let mut j = i + 3;
                    let mut found = Vec::new();
                    while j < toks.len() {
                        match &toks[j].0 {
                            Tok::Punct('{') => depth += 1,
                            Tok::Punct('}') => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            Tok::Str(s) => found.push(s.clone()),
                            _ => {}
                        }
                        j += 1;
                    }
                    if let [one] = found.as_slice() {
                        out.strings.insert(name.clone(), one.clone());
                    }
                }
            }
            Tok::Ident(m)
                if (m == "include_bytes" || m == "include_str" || m == "include")
                    && toks.get(i + 1).map(|t| &t.0) == Some(&Tok::Punct('!'))
                    && toks.get(i + 2).map(|t| &t.0) == Some(&Tok::Punct('(')) =>
            {
                let mut depth = 0;
                let mut j = i + 2;
                while j < toks.len() {
                    match toks[j].0 {
                        Tok::Punct('(') => depth += 1,
                        Tok::Punct(')') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let arg: Vec<Tok> = toks[i + 3..j].iter().map(|t| t.0.clone()).collect();
                let r = match gate {
                    Some(_) => Reach::Test,
                    None => here(&braces, whole),
                };
                let kind = match m.as_str() {
                    "include" => Macro::Code,
                    _ => Macro::Data,
                };
                out.includes.push((toks[i].1, arg, r, kind));
                i = j + 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// `a/b/../c` as `a/c`.
fn normal(p: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join("/")
}

/// What an include's argument tokens name, from the file `file` in crate
/// `krate`, with the file's one-string macros.
fn names(arg: &[Tok], file: &str, krate: &str, strings: &BTreeMap<String, String>) -> Names {
    let dir = Path::new(file).parent().unwrap_or(Path::new(""));
    let written = || {
        arg.iter()
            .map(|t| match t {
                Tok::Ident(s) => s.clone(),
                Tok::Str(s) => format!("{s:?}"),
                Tok::Punct(c) => c.to_string(),
            })
            .collect::<String>()
    };
    if let [Tok::Str(s)] = arg {
        return Names::Path(normal(&dir.join(s)));
    }
    let [
        Tok::Ident(c),
        Tok::Punct('!'),
        Tok::Punct('('),
        rest @ ..,
        Tok::Punct(')'),
    ] = arg
    else {
        return Names::Unread(written());
    };
    if c != "concat" {
        return Names::Unread(written());
    }
    let mut text = String::new();
    let mut base: Option<&str> = None;
    let mut k = 0;
    while k < rest.len() {
        match &rest[k..] {
            [Tok::Str(s), ..] => {
                text.push_str(s);
                k += 1;
            }
            [
                Tok::Ident(e),
                Tok::Punct('!'),
                Tok::Punct('('),
                Tok::Str(var),
                Tok::Punct(')'),
                ..,
            ] if e == "env" => {
                if !text.is_empty() || base.is_some() {
                    return Names::Unread(written());
                }
                base = match var.as_str() {
                    "CARGO_MANIFEST_DIR" => Some("manifest"),
                    "OUT_DIR" => Some("out"),
                    _ => return Names::Unread(written()),
                };
                k += 5;
            }
            [
                Tok::Ident(m),
                Tok::Punct('!'),
                Tok::Punct('('),
                Tok::Punct(')'),
                ..,
            ] => match strings.get(m) {
                Some(s) => {
                    text.push_str(s);
                    k += 4;
                }
                None => return Names::Unread(written()),
            },
            [Tok::Punct(','), ..] => k += 1,
            _ => return Names::Unread(written()),
        }
    }
    match base {
        Some("manifest") => {
            Names::Path(normal(&Path::new(krate).join(text.trim_start_matches('/'))))
        }
        Some(_) => Names::Built(format!("{krate}/OUT_DIR/{}", text.trim_start_matches('/'))),
        None => Names::Unread(written()),
    }
}

/// A crate manifest's path dependencies that the binary links: its normal
/// and target dependencies, not its build or dev ones, resolved through the
/// workspace's table.
fn linked(root: &Path, krate: &str, workspace: &Table) -> Result<Vec<String>, String> {
    let at = format!("{krate}/Cargo.toml");
    let t: Table = fs::read_to_string(root.join(&at))
        .map_err(|e| format!("{at}: {e}"))?
        .parse()
        .map_err(|e| format!("{at}: {e}"))?;
    let mut tables: Vec<&Table> = Vec::new();
    if let Some(Value::Table(d)) = t.get("dependencies") {
        tables.push(d);
    }
    if let Some(Value::Table(targets)) = t.get("target") {
        for v in targets.values() {
            if let Some(Value::Table(d)) = v.get("dependencies") {
                tables.push(d);
            }
        }
    }
    let mut out = Vec::new();
    for d in tables {
        for (name, spec) in d {
            let path = match spec.get("workspace") {
                Some(Value::Boolean(true)) => workspace
                    .get(name)
                    .and_then(|w| w.get("path"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                _ => spec
                    .get("path")
                    .and_then(Value::as_str)
                    .map(|p| normal(&Path::new(krate).join(p))),
            };
            if let Some(p) = path {
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// **The crates the binary is made of**: the app and every path crate it
/// links, directly or through another.
pub fn shipped_crates(root: &Path) -> Result<Vec<String>, String> {
    let top: Table = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("Cargo.toml: {e}"))?
        .parse()
        .map_err(|e| format!("Cargo.toml: {e}"))?;
    let empty = Table::new();
    let workspace = top
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table)
        .unwrap_or(&empty);
    let mut seen = BTreeSet::new();
    let mut todo = vec![APP.to_owned()];
    while let Some(k) = todo.pop() {
        if seen.insert(k.clone()) {
            todo.extend(linked(root, &k, workspace)?);
        }
    }
    Ok(seen.into_iter().collect())
}

/// A module's child file: `name.rs` or `name/mod.rs` beside a `mod.rs`,
/// `lib.rs` or `main.rs`, under its own directory otherwise.
fn child_file(root: &Path, parent: &str, c: &Child) -> Option<String> {
    let p = Path::new(parent);
    let dir = p.parent().unwrap_or(Path::new(""));
    if let Some(path) = &c.path {
        return Some(normal(&dir.join(path)));
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned());
    let base = match stem.as_deref() {
        Some("mod" | "lib" | "main") => dir.to_path_buf(),
        Some(s) => dir.join(s),
        None => return None,
    };
    [
        base.join(format!("{}.rs", c.name)),
        base.join(&c.name).join("mod.rs"),
    ]
    .into_iter()
    .map(|f| normal(&f))
    .find(|f| root.join(f).is_file())
}

/// Every `.rs` file under `dir`, from the repository root.
fn rust_files(root: &Path, dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_string_lossy().into_owned());
                }
            }
        }
    }
    out.sort();
    out
}

/// **Every include in the shipped crates' sources**, each with whether
/// test code alone reaches it. The modules are walked from each crate's
/// `src/lib.rs` and `src/main.rs`, so a module declared under a test gate
/// is test code throughout; a file under `src/` no declaration reaches is
/// read as shipped.
pub fn scan(root: &Path) -> Result<Vec<Include>, String> {
    let mut found = Vec::new();
    for krate in shipped_crates(root)? {
        let mut reach: BTreeMap<String, Reach> = BTreeMap::new();
        let mut todo: Vec<(String, Reach)> = ["lib.rs", "main.rs"]
            .iter()
            .map(|f| format!("{krate}/src/{f}"))
            .filter(|f| root.join(f).is_file())
            .map(|f| (f, Reach::Shipped))
            .collect();
        let mut reads: BTreeMap<String, Read> = BTreeMap::new();
        while let Some((f, r)) = todo.pop() {
            match reach.get(&f) {
                Some(Reach::Shipped) => continue,
                Some(Reach::Test) if r == Reach::Test => continue,
                _ => {}
            }
            reach.insert(f.clone(), r);
            let src = fs::read_to_string(root.join(&f)).map_err(|e| format!("{f}: {e}"))?;
            let read = read_file(&src, r);
            for c in &read.children {
                if let Some(child) = child_file(root, &f, c) {
                    let cr = match (r, c.reach) {
                        (Reach::Shipped, Reach::Shipped) => Reach::Shipped,
                        _ => Reach::Test,
                    };
                    todo.push((child, cr));
                }
            }
            reads.insert(f, read);
        }
        for f in rust_files(root, &format!("{krate}/src")) {
            if !reach.contains_key(&f) {
                let src = fs::read_to_string(root.join(&f)).map_err(|e| format!("{f}: {e}"))?;
                reads.insert(f, read_file(&src, Reach::Shipped));
            }
        }
        for (f, read) in &reads {
            for (line, arg, r, kind) in &read.includes {
                found.push(Include {
                    file: f.clone(),
                    line: *line,
                    names: names(arg, f, &krate, &read.strings),
                    reach: *r,
                    kind: *kind,
                });
            }
        }
    }
    Ok(found)
}

/// **The tables `baud-tablegen` writes as `include_bytes!`**: each build
/// file of a shipped crate that calls its `bytes`, with the data files
/// under the crate's `data/` it names, and those of the build modules it
/// takes its classes from (`use crate::grapheme::..`), through each of
/// theirs. The scan cannot see these includes, which are in code the build
/// writes, so they are found where they are asked for.
pub fn tables(root: &Path) -> Result<Vec<(String, Vec<String>)>, String> {
    let call = Regex::new(r#"\.bytes\(\s*"[A-Z0-9_]+""#).expect("the emit call");
    let lit = Regex::new(r#""([A-Za-z0-9._-]+\.(?:txt|TXT|h|c|ti|xml))""#).expect("a file name");
    let uses = Regex::new(r"\buse crate::([a-z_0-9]+)").expect("a use");
    let mut out = Vec::new();
    for krate in shipped_crates(root)? {
        let mut files = rust_files(root, &format!("{krate}/build"));
        if root.join(format!("{krate}/build.rs")).is_file() {
            files.push(format!("{krate}/build.rs"));
        }
        let mut src: BTreeMap<String, String> = BTreeMap::new();
        for f in &files {
            let text = fs::read_to_string(root.join(f)).map_err(|e| format!("{f}: {e}"))?;
            src.insert(f.clone(), text);
        }
        for f in &files {
            if !call.is_match(&src[f]) {
                continue;
            }
            let mut data: BTreeSet<String> = BTreeSet::new();
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut todo = vec![f.clone()];
            while let Some(g) = todo.pop() {
                if !seen.insert(g.clone()) {
                    continue;
                }
                let Some(text) = src.get(&g) else {
                    continue;
                };
                data.extend(
                    lit.captures_iter(text)
                        .map(|c| format!("{krate}/data/{}", &c[1]))
                        .filter(|p| root.join(p).exists()),
                );
                todo.extend(
                    uses.captures_iter(text)
                        .map(|c| format!("{krate}/build/{}.rs", &c[1])),
                );
            }
            out.push((f.clone(), data.into_iter().collect()));
        }
    }
    Ok(out)
}

/// **What a crate's build reads**, the inputs of the code it writes: every
/// file under the crate's `data/` and `protocols/`, which the builds read
/// whole, and every other file of the crate a build source names by a
/// string literal (`"xkbcommon-keysyms.h"`), as its `rerun-if-changed`
/// lines name them. Rust sources are the build's own code and are left out.
pub fn build_inputs(root: &Path, krate: &str) -> Result<Vec<String>, String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for dir in ["data", "protocols"] {
        let Ok(entries) = fs::read_dir(root.join(krate).join(dir)) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if root.join(krate).join(dir).join(&name).is_file() && !name.ends_with(".rs") {
                out.insert(format!("{krate}/{dir}/{name}"));
            }
        }
    }
    let lit = Regex::new(r#""([A-Za-z0-9._/-]+\.[A-Za-z0-9]+)""#).expect("a path literal");
    let mut sources = rust_files(root, &format!("{krate}/build"));
    if root.join(krate).join("build.rs").is_file() {
        sources.push(format!("{krate}/build.rs"));
    }
    for f in sources {
        let src = fs::read_to_string(root.join(&f)).map_err(|e| format!("{f}: {e}"))?;
        for c in lit.captures_iter(&src) {
            let rel = c[1].trim_start_matches('/');
            let path = normal(&Path::new(krate).join(rel));
            if !path.ends_with(".rs") && root.join(&path).is_file() && path.starts_with(krate) {
                out.insert(path);
            }
        }
    }
    Ok(out.into_iter().collect())
}

// ── The sidecar's credits ────────────────────────────────────────────────

/// The `[[tree]]` rows, as tables.
pub fn rows(side: &Table) -> Result<Vec<&Table>, String> {
    match side.get("tree") {
        Some(Value::Array(a)) => a
            .iter()
            .map(|t| t.as_table().ok_or_else(|| "a [[tree]] entry".to_owned()))
            .collect(),
        _ => Err(format!("{}: no [[tree]]", third_party::SIDECAR)),
    }
}

/// A key's strings: an array's, or one string's.
pub fn strs(t: &Table, key: &str) -> Vec<String> {
    match t.get(key) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn str_of(t: &Table, key: &str) -> Option<String> {
    t.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// The path, and the file it reaches through any link, from the root.
fn both(root: &Path, p: &str) -> Vec<String> {
    let mut out = vec![p.to_owned()];
    if let (Ok(real), Ok(top)) = (fs::canonicalize(root.join(p)), fs::canonicalize(root)) {
        if let Ok(rel) = real.strip_prefix(&top) {
            let rel = rel.to_string_lossy().into_owned();
            if rel != p {
                out.push(rel);
            }
        }
    }
    out
}

/// Text with every run of whitespace one space, and what copies of one
/// text differ by set aside: a Markdown copy's markup (`*AS IS*` for
/// `"AS IS"`), a rule of dashes under a heading, and a dash spelled `--`
/// in one copy and as an em dash in another, and a comment's `#` leading a
/// line of a data file's header.
pub fn flat(s: &str) -> String {
    s.replace(['*', '"', '\u{feff}'], " ")
        .split_whitespace()
        .filter(|w| {
            !w.chars()
                .all(|c| matches!(c, '-' | '=' | '#' | '\u{2013}' | '\u{2014}'))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A name ID of a font's `name` table: 0 is its copyright notice, 7 its
/// trademark notice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NameId(pub u16);

/// A font's name records of `id` in Windows Unicode English, the copy every
/// shipping font carries; each distinct text once.
pub fn font_names(bytes: &[u8], id: NameId) -> Vec<String> {
    let be16 = |o: usize| {
        bytes
            .get(o..o + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let be32 = |o: usize| {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let mut out: Vec<String> = Vec::new();
    let Some(tables) = be16(4) else {
        return out;
    };
    let Some(name) = (0..usize::from(tables))
        .map(|i| 12 + 16 * i)
        .find(|&o| bytes.get(o..o + 4) == Some(b"name"))
        .and_then(|o| be32(o + 8))
        .and_then(|o| usize::try_from(o).ok())
    else {
        return out;
    };
    let (Some(count), Some(strings)) = (be16(name + 2), be16(name + 4)) else {
        return out;
    };
    for r in 0..usize::from(count) {
        let at = name + 6 + 12 * r;
        let rec: Vec<u16> = (0..6).filter_map(|k| be16(at + 2 * k)).collect();
        if let [3, 1, 0x409, nid, len, off] = rec.as_slice() {
            if *nid != id.0 {
                continue;
            }
            let from = name + usize::from(strings) + usize::from(*off);
            if let Some(raw) = bytes.get(from..from + usize::from(*len)) {
                let units: Vec<u16> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes(*c))
                    .collect();
                let text = String::from_utf16_lossy(&units);
                if !out.contains(&text) {
                    out.push(text);
                }
            }
        }
    }
    out
}

/// An icon set, or another part a credit lists.
struct Part {
    name: String,
    by: Option<String>,
    copyright: Vec<String>,
    licences: Vec<String>,
    url: Option<String>,
    changes: Option<String>,
    note: Option<String>,
    /// A file printed whole inside the notice: an icon set's own licence
    /// statement.
    quote: Option<String>,
}

/// A permission notice read out of a work's own notice file: from the line
/// holding `from` (the first after the line holding `after`, where one is
/// given) to the line before the one holding `to`, or the file's end, with
/// the copyright lines that head it there.
struct Block {
    title: String,
    file: String,
    copyright: Vec<String>,
    /// The notice's words, their common indent taken off.
    body: String,
}

/// A row's credit, as the file prints it.
struct Credit {
    title: String,
    used: String,
    changes: String,
    copyright: Vec<String>,
    licences: Vec<String>,
    /// The permission notices its notice files give, for a work under no
    /// licence of [`LICENCES`] or under one in its own words.
    blocks: Vec<Block>,
    /// Why a work needs no licence, for one that is given under none.
    none: Option<String>,
    /// A sentence the licence asks the documentation to carry, word for word.
    statement: Option<String>,
    /// For code transcribed into baud's sources: the files it was ported from.
    ported: Vec<String>,
    /// Each font of the row the binary embeds, with its name IDs 0 and 7.
    fonts: Vec<(String, Vec<String>, Vec<String>)>,
    /// What the parts are, said once above them.
    parts_are: String,
    parts: Vec<Part>,
}

/// A notice file's words: an XML file's character references decoded, as
/// its reader sees them (`&lt;` is `<`).
fn notice_words(root: &Path, file: &str) -> Result<String, String> {
    let text = fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
    let text = text.replace("\r\n", "\n");
    Ok(match file.ends_with(".xml") {
        true => text
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
        false => text,
    })
}

/// The lines of `file` from the one holding `from` (searched from the line
/// after the one holding `after`) to the line before the one holding `to`,
/// or its end; their common indent taken off and blank lines trimmed.
fn block_text(
    root: &Path,
    file: &str,
    after: Option<&str>,
    from: &str,
    to: Option<&str>,
) -> Result<String, String> {
    let all = notice_words(root, file)?;
    let lines: Vec<&str> = all.lines().collect();
    let begin = match after {
        Some(a) => {
            lines
                .iter()
                .position(|s| s.contains(a))
                .ok_or_else(|| format!("{file}: no line holds {a:?}"))?
                + 1
        }
        None => 0,
    };
    let start = begin
        + lines[begin..]
            .iter()
            .position(|s| s.contains(from))
            .ok_or_else(|| format!("{file}: no line holds {from:?}"))?;
    let end = match to {
        Some(t) => lines[start + 1..]
            .iter()
            .position(|s| s.contains(t))
            .map(|k| start + 1 + k)
            .ok_or_else(|| format!("{file}: no line after {from:?} holds {t:?}"))?,
        None => lines.len(),
    };
    let body = &lines[start..end];
    let indent = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out: Vec<&str> = body
        .iter()
        .map(|l| l.get(indent..).unwrap_or("").trim_end())
        .collect();
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    Ok(out.join("\n"))
}

/// Each text `t` must be in one of `files`, whitespace aside.
fn found_in(root: &Path, t: &str, files: &[String], what: &str, errs: &mut Vec<String>) {
    let want = flat(t);
    let hit = files.iter().any(|f| {
        fs::read_to_string(root.join(f))
            .map(|s| flat(&s).contains(&want))
            .unwrap_or(false)
    });
    if !hit {
        errs.push(format!(
            "{what}: \"{t}\" is in none of {}",
            files.join(", ")
        ));
    }
}

/// A notice file under licence `key` must be that licence's: hold the
/// marker, and, where it carries the licence's text, the same text the file
/// prints.
fn notice_is(root: &Path, key: &str, notice: &str, what: &str, errs: &mut Vec<String>) {
    let Some(l) = licence(key) else {
        errs.push(format!(
            "{what}: no licence {key} (xtask/src/credits.rs, LICENCES)"
        ));
        return;
    };
    let Ok(text) = fs::read_to_string(root.join(notice)) else {
        errs.push(format!("{what}: {notice} is not there"));
        return;
    };
    if !l.marker.is_empty() && !flat(&text).contains(&flat(l.marker)) {
        errs.push(format!("{what}: {notice} does not say {:?}", l.marker));
    }
    if let (Ok(body), true) = (licence_text(root, l), matches!(key, "OFL-1.1" | "MIT")) {
        let first = body.lines().find(|s| !s.trim().is_empty()).unwrap_or("");
        let start = flat(first);
        let flat_text = flat(&text);
        if flat_text.contains(&start) && !flat_text.contains(&flat(&body)) {
            errs.push(format!(
                "{what}: {notice} holds a {key} text other than the one printed"
            ));
        }
    }
}

/// The credits of the rows `reached`, read and checked.
fn credits(
    root: &Path,
    side: &Table,
    reached: &BTreeMap<Count, BTreeSet<String>>,
    errs: &mut Vec<String>,
) -> Result<Vec<Credit>, String> {
    let mut out = Vec::new();
    for (i, row) in rows(side)?.into_iter().enumerate() {
        let what = format!("[[tree]] {i}");
        let credit = row.get("credit").and_then(Value::as_table);
        let embeds = reached.get(&Count(i));
        let (Some(c), Some(embeds)) = (credit, embeds) else {
            if credit.is_some() {
                errs.push(format!(
                    "{what} carries a credit and nothing the binary embeds falls under it"
                ));
            }
            if embeds.is_some() {
                errs.push(format!(
                    "{what} ({}) is embedded and carries no [tree.credit]",
                    row.get("what").and_then(Value::as_str).unwrap_or("?")
                ));
            }
            continue;
        };
        // A credit names its own notice file where the row's is "in file".
        let notice = str_of(c, "notice")
            .or_else(|| str_of(row, "notice"))
            .unwrap_or_default();
        let mut seen = vec![notice.clone()];
        seen.extend(strs(c, "seen"));
        let need = |k: &str, errs: &mut Vec<String>| {
            str_of(c, k).unwrap_or_else(|| {
                errs.push(format!("{what}: its credit has no {k}"));
                String::new()
            })
        };
        let title = need("title", errs);
        let used = need("used", errs);
        let changes = need("changes", errs);
        let licences = strs(c, "licence");
        let none = str_of(c, "none");
        let mut blocks = Vec::new();
        if let Some(Value::Array(a)) = c.get("text") {
            for (k, b) in a.iter().enumerate() {
                let bw = format!("{what}, text {k}");
                let Some(b) = b.as_table() else {
                    errs.push(format!("{bw}: not a table"));
                    continue;
                };
                let (Some(title), Some(file), Some(from)) =
                    (str_of(b, "title"), str_of(b, "file"), str_of(b, "from"))
                else {
                    errs.push(format!("{bw}: needs a title, a file and a from"));
                    continue;
                };
                let after = str_of(b, "after");
                let to = str_of(b, "to");
                let body = match block_text(root, &file, after.as_deref(), &from, to.as_deref()) {
                    Ok(t) => t,
                    Err(e) => {
                        errs.push(format!("{bw}: {e}"));
                        continue;
                    }
                };
                let copyright = strs(b, "copyright");
                let words = flat(&notice_words(root, &file).unwrap_or_default());
                for line in &copyright {
                    if !words.contains(&flat(line)) {
                        errs.push(format!("{bw}: \"{line}\" is not in {file}"));
                    }
                }
                blocks.push(Block {
                    title,
                    file,
                    copyright,
                    body,
                });
            }
        }
        let statement = str_of(c, "statement");
        if let Some(st) = &statement {
            let want = flat(st.trim_end_matches('.')).to_lowercase();
            let files: Vec<&String> = seen.iter().chain(blocks.iter().map(|b| &b.file)).collect();
            let hit = files.iter().any(|f| {
                fs::read_to_string(root.join(f))
                    .map(|t| flat(&t).to_lowercase().contains(&want))
                    .unwrap_or(false)
            });
            if !hit {
                errs.push(format!(
                    "{what}: its statement {st:?} is not the words of its notice"
                ));
            }
        }
        let ported = match row.get("transcribed").and_then(Value::as_bool) {
            Some(true) => strs(row, "ported_from"),
            _ => Vec::new(),
        };
        let copyright = strs(c, "copyright");
        for line in &copyright {
            found_in(root, line, &seen, &what, errs);
        }
        for key in &licences {
            notice_is(root, key, &notice, &what, errs);
        }
        let mut fonts = Vec::new();
        for p in embeds {
            if third_party::FONT_EXTENSIONS
                .iter()
                .any(|x| p.to_ascii_lowercase().ends_with(&format!(".{x}")))
            {
                let bytes = fs::read(root.join(p)).map_err(|e| format!("{p}: {e}"))?;
                let ids0 = font_names(&bytes, NameId(0));
                if ids0.is_empty() {
                    errs.push(format!("{p}: no copyright notice (name ID 0) in the font"));
                }
                fonts.push((p.clone(), ids0, font_names(&bytes, NameId(7))));
            }
        }
        let mut parts = Vec::new();
        if let Some(Value::Array(a)) = c.get("part") {
            for (k, p) in a.iter().enumerate() {
                let Some(p) = p.as_table() else {
                    errs.push(format!("{what}: part {k} is not a table"));
                    continue;
                };
                let pw = format!("{what}, part {k}");
                let mut pseen = seen.clone();
                pseen.extend(strs(p, "seen"));
                let pnotice = str_of(p, "notice");
                if let Some(n) = &pnotice {
                    pseen.push(n.clone());
                }
                let part = Part {
                    name: str_of(p, "name").unwrap_or_else(|| {
                        errs.push(format!("{pw}: no name"));
                        String::new()
                    }),
                    by: str_of(p, "by"),
                    copyright: strs(p, "copyright"),
                    licences: strs(p, "licence"),
                    url: str_of(p, "url"),
                    changes: str_of(p, "changes"),
                    note: str_of(p, "note"),
                    quote: str_of(p, "quote"),
                };
                for t in part
                    .copyright
                    .iter()
                    .chain(part.by.iter())
                    .chain(part.url.iter())
                {
                    found_in(root, t, &pseen, &pw, errs);
                }
                if part.licences.is_empty() {
                    errs.push(format!("{pw}: names no licence"));
                }
                for key in &part.licences {
                    match &pnotice {
                        Some(n) => notice_is(root, key, n, &pw, errs),
                        None if licence(key).is_none() => {
                            errs.push(format!("{pw}: no licence {key}"));
                        }
                        None => {}
                    }
                    if licence(key).is_some_and(|l| l.disclaimer.is_some()) {
                        if part.by.is_none() || part.url.is_none() || part.changes.is_none() {
                            errs.push(format!(
                                "{pw}: a CC BY work needs its creator, its URI and its changes"
                            ));
                        }
                        if part.copyright.is_empty() && part.note.is_none() {
                            errs.push(format!(
                                "{pw}: a CC BY work with no copyright notice says so in a note"
                            ));
                        }
                    }
                }
                if let Some(q) = &part.quote {
                    if !root.join(q).is_file() {
                        errs.push(format!("{pw}: {q} is not there"));
                    }
                }
                parts.push(part);
            }
        }
        if licences.is_empty() && blocks.is_empty() && none.is_none() && parts.is_empty() {
            errs.push(format!(
                "{what}: its credit names no licence, gives no text, has no parts and says \
                 none is needed"
            ));
        }
        let parts_are = str_of(c, "parts_are").unwrap_or_default();
        if !parts.is_empty() && parts_are.is_empty() {
            errs.push(format!("{what}: its credit has parts and no parts_are"));
        }
        out.push(Credit {
            title,
            used,
            changes,
            copyright,
            licences,
            blocks,
            none,
            statement,
            ported,
            fonts,
            parts_are,
            parts,
        });
    }
    Ok(out)
}

// ── What falls where ─────────────────────────────────────────────────────

/// What the scan checks the tree against: [`OWN`], [`PENDING`] and its
/// controls, for the tree; a test hands it a small tree's own.
pub struct Lists<'a> {
    pub own: &'a [Own],
    pub pending: &'a [Pending],
    pub must_ship: &'a [&'a str],
    pub must_test: &'a [&'a str],
    pub computed: &'a [Computed],
}

/// The tree's lists.
pub const LISTS: Lists<'static> = Lists {
    own: &OWN,
    pending: &PENDING,
    must_ship: &MUST_SHIP,
    must_test: &MUST_TEST,
    computed: &COMPUTED,
};

/// The rows the binary reaches, each with the files that reach it, and
/// every include that falls under nothing.
pub struct Placed {
    pub reached: BTreeMap<Count, BTreeSet<String>>,
    pub errors: Vec<String>,
    /// The shipped includes, by the file they name.
    pub shipped: BTreeSet<String>,
    /// The includes only tests reach.
    pub test: BTreeSet<String>,
}

/// Every include and table placed under a row, [`OWN`] or [`PENDING`].
pub fn place(root: &Path, side: &Table) -> Result<Placed, String> {
    place_with(root, side, &LISTS)
}

/// [`place`], against `lists`.
pub fn place_with(root: &Path, side: &Table, lists: &Lists<'_>) -> Result<Placed, String> {
    let rows = rows(side)?;
    let row_of = |path: &str| -> Vec<Count> {
        rows.iter()
            .enumerate()
            .filter(|(_, r)| strs(r, "paths").iter().any(|p| p == path))
            .map(|(i, _)| Count(i))
            .collect()
    };
    let mut p = Placed {
        reached: BTreeMap::new(),
        errors: Vec::new(),
        shipped: BTreeSet::new(),
        test: BTreeSet::new(),
    };
    let reach = |rows: Vec<Count>, file: &str, p: &mut Placed| {
        for r in rows {
            p.reached.entry(r).or_default().insert(file.to_owned());
        }
    };
    let own_from = |o: &Own, p: &mut Placed| -> Vec<Count> {
        let mut out = Vec::new();
        for f in o.from {
            let r = row_of(f);
            if r.is_empty() {
                p.errors.push(format!(
                    "OWN {}: no row names {f} (xtask/src/credits.rs)",
                    o.path
                ));
            }
            out.extend(r);
        }
        out
    };
    let mut generated: BTreeSet<String> = BTreeSet::new();
    for inc in scan(root)? {
        let at = format!("{}:{}", inc.file, inc.line.0);
        if inc.kind == Macro::Code {
            // Rust compiled in: a module of the crate's own, or code its
            // build wrote, whose inputs are followed below.
            if let (Names::Built(_), Reach::Shipped) = (&inc.names, inc.reach) {
                if let Some(krate) = inc.file.split('/').next() {
                    generated.insert(krate.to_owned());
                }
            }
            continue;
        }
        let path = match &inc.names {
            Names::Path(path) | Names::Built(path) => path.clone(),
            Names::Unread(text) => {
                if inc.reach == Reach::Shipped {
                    p.errors
                        .push(format!("{at}: an include the scan cannot read: {text}"));
                }
                continue;
            }
        };
        if inc.reach == Reach::Test {
            p.test.insert(path);
            continue;
        }
        p.shipped.insert(path.clone());
        let forms = both(root, &path);
        let rowed: Vec<Count> = forms.iter().flat_map(|f| row_of(f)).collect();
        if !rowed.is_empty() {
            reach(rowed, &path, &mut p);
            continue;
        }
        if let Some(o) = lists.own.iter().find(|o| forms.iter().any(|f| f == o.path)) {
            let r = own_from(o, &mut p);
            reach(r, &path, &mut p);
            continue;
        }
        if let Some(pend) = lists.pending.iter().find(|x| x.path == path) {
            if root.join(&path).exists() {
                p.errors.push(format!(
                    "{at}: {path} is in the tree now, so it needs its row in {}, with {} as \
                     its notice",
                    third_party::SIDECAR,
                    pend.notice
                ));
            }
            continue;
        }
        p.errors.push(format!(
            "{at}: {path} is embedded and falls under no row of {} and is not one of baud's \
             own files (OWN in xtask/src/credits.rs)",
            third_party::SIDECAR
        ));
    }
    // G1: what each build that writes compiled-in code read.
    for krate in &generated {
        let inputs = build_inputs(root, krate)?;
        if inputs.is_empty() {
            p.errors.push(format!(
                "{krate}'s build writes code the binary compiles in, and no input of it was found"
            ));
        }
        for input in inputs {
            let label = format!("{krate}'s build reads {input}");
            let forms = both(root, &input);
            let rowed: Vec<Count> = forms.iter().flat_map(|f| row_of(f)).collect();
            if !rowed.is_empty() {
                p.shipped.insert(label.clone());
                reach(rowed, &label, &mut p);
            } else if let Some(o) = lists.own.iter().find(|o| forms.iter().any(|f| f == o.path)) {
                p.shipped.insert(label.clone());
                let r = own_from(o, &mut p);
                reach(r, &label, &mut p);
            } else {
                p.errors.push(format!(
                    "{label}, compiled into the binary, and it falls under no row of {} and is \
                     not one of baud's own files (OWN in xtask/src/credits.rs)",
                    third_party::SIDECAR
                ));
            }
        }
    }
    // Every input a shipped crate's build says it reads is one the scan
    // follows, a row's or baud's own.
    let shipped_crates = shipped_crates(root)?;
    for krate in &shipped_crates {
        let inputs = build_inputs(root, krate)?;
        for pr in build_prints(root, krate)? {
            let said = format!("{} prints rerun-if-changed={}", pr.file, pr.printed);
            match pr.printed.split_once('{') {
                Some(("", _)) => {
                    let arg = pr.arg.clone().unwrap_or_default();
                    if !lists
                        .computed
                        .iter()
                        .any(|c| c.file == pr.file && c.arg == arg)
                    {
                        p.errors.push(format!(
                            "{said} of {arg}, a value no entry of COMPUTED in \
                             xtask/src/credits.rs says what it is"
                        ));
                    }
                }
                Some((dir, _)) => {
                    let dir = dir.trim_end_matches('/');
                    if !matches!(dir, "data" | "protocols") {
                        p.errors.push(format!(
                            "{said}, a file under {krate}/{dir}, a folder the scan does not \
                             read whole"
                        ));
                    }
                }
                None => {
                    let path = normal(&Path::new(krate).join(&pr.printed));
                    let own_code = matches!(pr.printed.as_str(), "build.rs" | "build")
                        || pr.printed.ends_with(".rs")
                        || pr.printed.starts_with("src/");
                    let forms = both(root, &path);
                    let known = own_code
                        || inputs.contains(&path)
                        || forms.iter().any(|f| !row_of(f).is_empty())
                        || lists.own.iter().any(|o| forms.iter().any(|f| f == o.path));
                    if !known {
                        p.errors.push(format!(
                            "{said}: {path} is no input the scan follows, falls under no row \
                             of {} and is not one of baud's own files",
                            third_party::SIDECAR
                        ));
                    }
                }
            }
        }
    }
    // G2: code transcribed into baud's own sources.
    for (i, row) in rows.iter().enumerate() {
        if row.get("transcribed").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        // Where a file ported from is named: the row's own words, its
        // upstream and sources, or its notice file.
        let mut said = str_of(row, "what").unwrap_or_default();
        for k in ["upstream", "from"] {
            for t in strs(row, k) {
                said.push('\n');
                said.push_str(&t);
            }
        }
        if let Some(n) = str_of(row, "notice") {
            said.push_str(&fs::read_to_string(root.join(n)).unwrap_or_default());
        }
        let ported = strs(row, "ported_from");
        if ported.is_empty() {
            p.errors.push(format!(
                "[[tree]] {i} is transcribed and names no ported_from"
            ));
        }
        for f in &ported {
            if !said.contains(f.as_str()) {
                p.errors.push(format!(
                    "[[tree]] {i}: ported_from {f:?} is named by neither its what, its \
                     upstream nor its notice"
                ));
            }
        }
        let shipped: Vec<String> = strs(row, "paths")
            .into_iter()
            .filter(|path| {
                shipped_crates.iter().any(|k| {
                    path.starts_with(&format!("{k}/src/"))
                        || path.starts_with(&format!("{k}/build/"))
                }) && !path.ends_with("/tests.rs")
            })
            .collect();
        if shipped.is_empty() {
            p.errors.push(format!(
                "[[tree]] {i} is transcribed and names no source of a crate the binary is made of"
            ));
        }
        let label = format!("transcribed into {}", shipped.join(", "));
        p.shipped.insert(label.clone());
        reach(vec![Count(i)], &label, &mut p);
    }
    for (file, data) in tables(root)? {
        let rowed: Vec<Count> = data.iter().flat_map(|d| row_of(d)).collect();
        if rowed.is_empty() {
            p.errors.push(format!(
                "{file} writes a table the binary embeds, from no data file a row names"
            ));
        }
        p.shipped.insert(format!("{file} (tables)"));
        reach(rowed, &format!("{file} (tables)"), &mut p);
    }
    for o in lists.own {
        if !p
            .shipped
            .iter()
            .any(|s| both(root, s).iter().any(|f| f == o.path))
        {
            p.errors.push(format!(
                "OWN {}: no shipped include names it any more (xtask/src/credits.rs)",
                o.path
            ));
        }
    }
    for must in lists.must_ship {
        if !p.shipped.contains(*must) {
            p.errors.push(format!(
                "the control: {must} is not found shipped, so the scan is not reading includes"
            ));
        }
    }
    for must in lists.must_test {
        if !p.test.contains(*must) {
            p.errors.push(format!(
                "the control: {must} is not found in test code, so the scan is not telling \
                 tests apart"
            ));
        }
    }
    Ok(p)
}

// ── The file ─────────────────────────────────────────────────────────────

/// The width the prose is wrapped to.
const WIDTH: usize = 76;

/// `text` wrapped to [`WIDTH`], its first line after `lead` and the rest
/// under it.
fn wrapped(out: &mut String, lead: &str, text: &str) {
    let indent = " ".repeat(lead.chars().count());
    let mut line = lead.to_owned();
    let mut fresh = true;
    for w in text.split_whitespace() {
        let len = line.chars().count();
        if !fresh && len + 1 + w.chars().count() > WIDTH {
            out.push_str(line.trim_end());
            out.push('\n');
            line = indent.clone();
            fresh = true;
        }
        if !fresh {
            line.push(' ');
        }
        line.push_str(w);
        fresh = false;
    }
    out.push_str(line.trim_end());
    out.push('\n');
}

/// The licence text, as the file prints it.
pub fn licence_text(root: &Path, l: &Licence) -> Result<String, String> {
    match &l.source {
        Source::Uri { uri, seen, .. } => {
            let text = fs::read_to_string(root.join(seen)).map_err(|e| format!("{seen}: {e}"))?;
            match text.contains(uri) {
                true => Ok(uri.to_string()),
                false => Err(format!("{seen} does not spell {uri}")),
            }
        }
        Source::File { path, from, to } => {
            let all = fs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
            let all = all.replace("\r\n", "\n");
            let lines: Vec<&str> = all.lines().collect();
            let start = match from {
                Some(f) => lines
                    .iter()
                    .position(|s| s.contains(f))
                    .ok_or_else(|| format!("{path}: no line holds {f:?}"))?,
                None => 0,
            };
            let end = match to {
                Some(t) => lines[start..]
                    .iter()
                    .position(|s| s.contains(t))
                    .map_or(lines.len(), |k| start + k),
                None => lines.len(),
            };
            let body: Vec<&str> = lines[start..end].to_vec();
            let mut text = body.join("\n");
            while text.ends_with('\n') {
                text.pop();
            }
            Ok(text)
        }
    }
}

/// How a licence text's words are printed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    /// Prose the file wraps: a licence given by its URI.
    Prose,
    /// A text printed as it stands, line for line.
    AsGiven,
}

/// A licence text the file prints once.
struct Text {
    title: String,
    /// Its words; for a licence given by its URI, the sentence and the URI.
    body: String,
    form: Form,
    /// The works under it: the number of each one's notice, and each of its
    /// copyright lines, or its name where it gives none.
    under: Vec<(String, String)>,
}

/// **Each distinct text once.** Two works whose permission notices hold the
/// same words, whitespace and markup aside, share one text, and every
/// copyright line of each is listed above it.
#[derive(Default)]
struct Texts {
    all: Vec<Text>,
    at: BTreeMap<String, Count>,
}

impl Texts {
    fn add(&mut self, title: &str, body: String, form: Form) -> Count {
        let key = flat(&body);
        if let Some(&i) = self.at.get(&key) {
            return i;
        }
        let i = Count(self.all.len());
        self.all.push(Text {
            title: title.to_owned(),
            body,
            form,
            under: Vec::new(),
        });
        self.at.insert(key, i);
        i
    }

    fn under(&mut self, i: Count, label: &str, lines: &[String], name: &str) {
        let t = &mut self.all[i.0];
        let name = [name.to_owned()];
        let lines = match lines.is_empty() {
            true => &name[..],
            false => lines,
        };
        for l in lines {
            let e = (label.to_owned(), l.clone());
            if !t.under.contains(&e) {
                t.under.push(e);
            }
        }
    }

    /// How a notice names text `i`.
    fn named(&self, i: Count) -> String {
        let t = &self.all[i.0];
        let at = letter(i);
        match t.form {
            Form::Prose => format!("{} ({at}, below)", t.title),
            Form::AsGiven => format!("{} (full text: {at}, below)", t.title),
        }
    }
}

const RULE: &str = "============================================================================";
const THIN: &str = "----------------------------------------------------------------------------";

/// **The file**, from the credits and the texts they name.
fn render(root: &Path, credits: &[Credit], links: &[crates::Row]) -> Result<String, String> {
    let mut keys: BTreeSet<&str> = BTreeSet::new();
    keys.extend(links.iter().map(|r| r.licence.as_str()));
    for c in credits {
        keys.extend(c.licences.iter().map(String::as_str));
        for p in &c.parts {
            keys.extend(p.licences.iter().map(String::as_str));
        }
    }
    for l in LICENCES.iter().filter(|l| keys.contains(l.key)) {
        if let Some((words, seen)) = l.disclaimer {
            let text = fs::read_to_string(root.join(seen)).map_err(|e| format!("{seen}: {e}"))?;
            if !flat(&text).contains(&flat(words)) {
                return Err(format!(
                    "{seen} does not name {}'s disclaimer {words:?}",
                    l.key
                ));
            }
        }
    }
    // The licences' texts first, in LICENCES' order, then the notices'
    // own texts as they come.
    let mut texts = Texts::default();
    let mut keyed: BTreeMap<&str, Count> = BTreeMap::new();
    for l in LICENCES.iter().filter(|l| keys.contains(l.key)) {
        let (body, form) = match &l.source {
            Source::Uri { why, .. } => (format!("{why} {}", licence_text(root, l)?), Form::Prose),
            Source::File { .. } => (licence_text(root, l)?, Form::AsGiven),
        };
        keyed.insert(l.key, texts.add(l.title, body, form));
    }
    let mut own: Vec<Vec<Count>> = Vec::new();
    for (n, c) in credits.iter().enumerate() {
        let label = format!("{}.", n + 1);
        for key in &c.licences {
            if let Some(&i) = keyed.get(key.as_str()) {
                texts.under(i, &label, &c.copyright, &c.title);
            }
        }
        let mut mine = Vec::new();
        for b in &c.blocks {
            let i = texts.add(&b.title, b.body.clone(), Form::AsGiven);
            texts.under(i, &label, &b.copyright, &c.title);
            mine.push(i);
        }
        own.push(mine);
        for (k, p) in c.parts.iter().enumerate() {
            let label = format!("{}.{}", n + 1, k + 1);
            for key in &p.licences {
                if let Some(&i) = keyed.get(key.as_str()) {
                    texts.under(i, &label, &p.copyright, &p.name);
                }
            }
        }
    }
    // The crates are one notice after the rows', each crate a part of it.
    let at = Count(credits.len() + 1);
    for (k, r) in links.iter().enumerate() {
        let i = keyed.get(r.licence.as_str()).copied().ok_or_else(|| {
            format!(
                "{} {}: no licence {} (xtask/src/credits.rs, LICENCES)",
                r.name, r.version, r.licence
            )
        })?;
        let label = format!("{}.{}", at.0, k + 1);
        texts.under(
            i,
            &label,
            &r.copyright,
            &format!("{} {}", r.name, r.version),
        );
    }
    let named = |key: &str| match keyed.get(key) {
        Some(&i) => texts.named(i),
        None => key.to_owned(),
    };
    let mut o = String::new();
    o.push_str(HEADER);
    o.push_str("\n\n");
    wrapped(
        &mut o,
        "",
        "baud's program file carries the works below. Each notice says what the work is, \
         where baud uses it, its copyright notices, what was changed, and the licence text \
         it falls under. The licence texts follow the notices: each text once, with the \
         copyright notices of every work under it listed above it.",
    );
    o.push('\n');
    o.push_str("Notices\n");
    for (n, c) in credits.iter().enumerate() {
        wrapped(&mut o, &format!("  {}. ", n + 1), &c.title);
    }
    if !links.is_empty() {
        wrapped(&mut o, &format!("  {}. ", at.0), CRATES_TITLE);
    }
    o.push_str("Licence texts\n");
    for (i, t) in texts.all.iter().enumerate() {
        wrapped(&mut o, &format!("  {}. ", letter(Count(i))), &t.title);
    }
    for (n, c) in credits.iter().enumerate() {
        o.push('\n');
        o.push_str(RULE);
        o.push('\n');
        wrapped(&mut o, &format!("{}. ", n + 1), &c.title);
        o.push_str(RULE);
        o.push('\n');
        wrapped(&mut o, "Used for:   ", &c.used);
        if !c.ported.is_empty() {
            wrapped(&mut o, "Ported from: ", &c.ported.join(", "));
        }
        let mut lines: Vec<&String> = Vec::new();
        for l in c
            .copyright
            .iter()
            .chain(c.blocks.iter().flat_map(|b| &b.copyright))
        {
            if !lines.contains(&l) {
                lines.push(l);
            }
        }
        for (k, line) in lines.iter().enumerate() {
            let lead = if k == 0 {
                "Copyright:  "
            } else {
                "            "
            };
            wrapped(&mut o, lead, line);
        }
        let mut said: Vec<(String, Vec<String>)> = Vec::new();
        for (file, ids0, ids7) in &c.fonts {
            let name = Path::new(file)
                .file_name()
                .map_or(file.clone(), |f| f.to_string_lossy().into_owned());
            for (id, texts) in [
                ("copyright notice, name ID 0", ids0),
                ("trademark notice, name ID 7", ids7),
            ] {
                for t in texts {
                    let key = format!("{id}\u{0}{t}");
                    match said.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, files)) => files.push(name.clone()),
                        None => said.push((key, vec![name.clone()])),
                    }
                }
            }
        }
        for (key, files) in &said {
            let (id, t) = key.split_once('\u{0}').unwrap_or((key, ""));
            wrapped(
                &mut o,
                "In the font: ",
                &format!("{}'s {id}: {t}", files.join(", ")),
            );
        }
        wrapped(&mut o, "Changes:    ", &c.changes);
        let mut under: Vec<String> = c.licences.iter().map(|k| named(k)).collect();
        for &i in &own[n] {
            let t = texts.named(i);
            if !under.contains(&t) {
                under.push(t);
            }
        }
        if let Some(none) = &c.none {
            under.push(format!("none is needed: {none}"));
        }
        for (k, t) in under.iter().enumerate() {
            let lead = if k == 0 {
                "Licence:    "
            } else {
                "            "
            };
            wrapped(&mut o, lead, t);
        }
        if let Some(st) = &c.statement {
            wrapped(&mut o, "As its licence asks: ", st);
        }
        if !c.parts.is_empty() {
            o.push('\n');
            wrapped(&mut o, "", &c.parts_are);
        }
        for (k, p) in c.parts.iter().enumerate() {
            o.push('\n');
            wrapped(&mut o, &format!("{}.{} ", n + 1, k + 1), &p.name);
            let cc = p
                .licences
                .iter()
                .filter_map(|l| licence(l))
                .find(|l| l.disclaimer.is_some());
            if let Some(by) = &p.by {
                wrapped(
                    &mut o,
                    if cc.is_some() {
                        "    Creator:           "
                    } else {
                        "    By:                "
                    },
                    by,
                );
            }
            if cc.is_some() {
                let notice = match p.copyright.as_slice() {
                    [] => p.note.clone().unwrap_or_default(),
                    lines => lines.join(" "),
                };
                wrapped(&mut o, "    Copyright notice:  ", &notice);
            } else {
                for (j, line) in p.copyright.iter().enumerate() {
                    let lead = if j == 0 {
                        "    Copyright:         "
                    } else {
                        "                       "
                    };
                    wrapped(&mut o, lead, line);
                }
            }
            for (j, key) in p.licences.iter().enumerate() {
                let lead = if j == 0 {
                    "    Licence:           "
                } else {
                    "                       "
                };
                wrapped(&mut o, lead, &named(key));
            }
            if let Some(Licence {
                disclaimer: Some((words, _)),
                ..
            }) = cc
            {
                wrapped(
                    &mut o,
                    "    Disclaimer:        ",
                    &format!("The licence's disclaimer applies: its \"{words}\"."),
                );
            }
            if let Some(u) = &p.url {
                wrapped(
                    &mut o,
                    if cc.is_some() {
                        "    Material:          "
                    } else {
                        "    Upstream:          "
                    },
                    u,
                );
            }
            if let Some(ch) = &p.changes {
                wrapped(&mut o, "    Changes:           ", ch);
            }
            if let Some(Licence {
                source: Source::Uri { uri, .. },
                ..
            }) = cc
            {
                wrapped(&mut o, "    Licence URI:       ", uri);
            }
            if let (Some(note), false) = (&p.note, cc.is_some() && p.copyright.is_empty()) {
                wrapped(&mut o, "    Note:              ", note);
            }
            if let Some(q) = &p.quote {
                let text = fs::read_to_string(root.join(q)).map_err(|e| format!("{q}: {e}"))?;
                o.push_str("    Its licence statement, as it gives it:\n\n");
                for line in text.trim_end().lines() {
                    if line.is_empty() {
                        o.push('\n');
                    } else {
                        o.push_str("      ");
                        o.push_str(line);
                        o.push('\n');
                    }
                }
            }
        }
    }
    if !links.is_empty() {
        crate_notices(root, &mut o, at, links, &named)?;
    }
    o.push('\n');
    o.push_str(RULE);
    o.push_str("\nLicence texts\n");
    o.push_str(RULE);
    o.push('\n');
    for (i, t) in texts.all.iter().enumerate() {
        o.push('\n');
        o.push_str(THIN);
        o.push('\n');
        wrapped(&mut o, &format!("{}. ", letter(Count(i))), &t.title);
        o.push_str(THIN);
        o.push('\n');
        if !t.under.is_empty() {
            o.push_str("The works under it, by notice, and their copyright notices:\n");
            let mut last = "";
            for (label, line) in &t.under {
                let lead = match label.as_str() == last {
                    true => "          ".to_owned(),
                    false => format!("  {label:<8}"),
                };
                wrapped(&mut o, &lead, line);
                last = label;
            }
            o.push('\n');
        }
        match t.form {
            Form::Prose => wrapped(&mut o, "", &t.body),
            Form::AsGiven => {
                o.push_str(&t.body);
                o.push('\n');
            }
        }
    }
    Ok(o)
}

/// The title of the crates' notice.
pub const CRATES_TITLE: &str = "Rust crates the program links";

/// **The crates' notice**, numbered `at`: per crate its name and version,
/// the platforms whose build links it, its copyright lines, and the
/// licence it falls under, named by `named`.
fn crate_notices(
    root: &Path,
    o: &mut String,
    at: Count,
    links: &[crates::Row],
    named: &dyn Fn(&str) -> String,
) -> Result<(), String> {
    o.push('\n');
    o.push_str(RULE);
    o.push('\n');
    wrapped(o, &format!("{}. ", at.0), CRATES_TITLE);
    o.push_str(RULE);
    o.push('\n');
    wrapped(
        o,
        "",
        "The crates from crates.io the program file links, each with the platforms whose \
         build links it, the copyright notices of its licence file, and \
         the licence it falls under. A crate offered under several licences is credited \
         under one of them: MIT, then BSD-2-Clause, then Apache-2.0.",
    );
    for (k, r) in links.iter().enumerate() {
        o.push('\n');
        wrapped(
            o,
            &format!("{}.{} ", at.0, k + 1),
            &format!("{} {}", r.name, r.version),
        );
        let mut platforms: Vec<&str> = Vec::new();
        for t in crates::TARGETS
            .iter()
            .filter(|t| r.targets.iter().any(|x| x == t.triple))
        {
            if !platforms.contains(&t.platform) {
                platforms.push(t.platform);
            }
        }
        wrapped(o, "    Platforms:         ", &platforms.join(", "));
        for (j, line) in r.copyright.iter().enumerate() {
            let lead = if j == 0 {
                "    Copyright:         "
            } else {
                "                       "
            };
            wrapped(o, lead, line);
        }
        if !r.notice_source.is_empty() {
            let c = crates::committed(root, &r.notice_source)
                .map_err(|e| format!("{} {}: {e}", r.name, r.version))?;
            wrapped(
                o,
                "    Note:              ",
                &format!(
                    "Its crates.io source carries no licence file; its copyright notice is \
                     from its upstream repository's licence file, {}.",
                    c.url
                ),
            );
        } else if r.copyright.is_empty() {
            for (j, a) in r.authors.iter().enumerate() {
                let lead = if j == 0 {
                    "    Authors:           "
                } else {
                    "                       "
                };
                wrapped(o, lead, a);
            }
            wrapped(
                o,
                "    Note:              ",
                "Its source carries no licence file and no copyright notice; its manifest \
                 names its licence, and its authors where it gives them.",
            );
        }
        if !r.repository.is_empty() {
            wrapped(o, "    Upstream:          ", &r.repository);
        }
        wrapped(o, "    Licence:           ", &named(&r.licence));
        if r.expression != r.licence {
            wrapped(o, "    Offered under:     ", &r.expression);
        }
        if !r.apache_notice_text.is_empty() {
            o.push_str("    Its NOTICE file, as it gives it:\n\n");
            for line in r.apache_notice_text.lines() {
                if line.is_empty() {
                    o.push('\n');
                } else {
                    o.push_str("      ");
                    o.push_str(line);
                    o.push('\n');
                }
            }
        }
    }
    Ok(())
}

/// Each section's title and size, largest first: what the tripwire prints.
pub fn sections(text: &str) -> Vec<(String, Bytes)> {
    let mut out: Vec<(String, Bytes)> = Vec::new();
    let mut title = String::from("(the head)");
    let mut size = 0;
    let lines: Vec<&str> = text.lines().collect();
    let ruled = |l: &str| l == RULE || l == THIN;
    // A heading is "12. " or "AB. " and its title, on one line or two.
    let heading = |l: &str| {
        l.split_once(". ").is_some_and(|(n, _)| {
            !n.is_empty()
                && (n.bytes().all(|b| b.is_ascii_digit())
                    || n.bytes().all(|b| b.is_ascii_uppercase()))
        })
    };
    for (k, line) in lines.iter().enumerate() {
        let next = lines.get(k + 1).copied().unwrap_or("");
        let end = (k + 2..=k + 3).find(|&j| lines.get(j).is_some_and(|l| ruled(l)));
        if let (true, true, Some(j)) = (ruled(line), heading(next), end) {
            out.push((title.clone(), Bytes(size)));
            title = lines[k + 1..j]
                .iter()
                .map(|s| s.trim())
                .collect::<Vec<_>>()
                .join(" ");
            size = 0;
        }
        size += line.len() + 1;
    }
    out.push((title, Bytes(size)));
    out.sort_by_key(|s| std::cmp::Reverse(s.1));
    out
}

/// **The tripwire**: `None` under [`TRIPWIRE`], else what to print.
pub fn tripwire(text: &str, limit: Bytes) -> Option<String> {
    if text.len() <= limit.0 {
        return None;
    }
    let mut s = format!(
        "{OUT} is {} bytes, past the {}-byte tripwire: something large was added. Review it, \
         and raise TRIPWIRE in xtask/src/credits.rs in a commit that says why; never cut \
         required text to fit. The largest sections:",
        text.len(),
        limit.0
    );
    for (t, b) in sections(text).iter().take(5) {
        let _ = write!(s, "\n         {} bytes  {t}", b.0);
    }
    Some(s)
}

/// **The file the tree generates**, or every reason it cannot.
pub fn generate(root: &Path) -> Result<String, Vec<String>> {
    let side = third_party::load(root, third_party::SIDECAR).map_err(|e| vec![e])?;
    let placed = place(root, &side).map_err(|e| vec![e])?;
    let mut errs = placed.errors;
    let credits = credits(root, &side, &placed.reached, &mut errs).map_err(|e| vec![e])?;
    if !errs.is_empty() {
        return Err(errs);
    }
    let links = crates::load(root).map_err(|e| vec![e])?;
    render(root, &credits, &links).map_err(|e| vec![e])
}

/// Whether the command writes the file or checks it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Write,
    Check,
    /// Every embedded file and the row it falls under, and nothing written.
    List,
    /// The register of the crates the program links, written from
    /// `Cargo.lock` and the crates' sources in the cargo registry.
    Crates,
}

/// `cargo xtask credits [--check|--list|--crates]`, run against `root`.
pub fn run(root: &Path, mode: Mode, out: &mut impl Write) -> io::Result<Outcome> {
    match mode {
        Mode::List => return list(root, out),
        Mode::Crates => return write_crates(root, out),
        Mode::Write | Mode::Check => {}
    }
    if crate_checks(root, out)? == Outcome::Failed {
        return Ok(Outcome::Failed);
    }
    let text = match generate(root) {
        Ok(t) => t,
        Err(errs) => {
            for e in errs {
                writeln!(out, "  FAIL {e}")?;
            }
            return Ok(Outcome::Failed);
        }
    };
    if let Some(e) = tripwire(&text, TRIPWIRE) {
        writeln!(out, "  FAIL {e}")?;
        return Ok(Outcome::Failed);
    }
    match mode {
        Mode::List | Mode::Crates => Ok(Outcome::Passed),
        Mode::Write => {
            fs::write(root.join(OUT), &text)?;
            writeln!(out, "  ok   wrote {OUT}, {} bytes", text.len())?;
            Ok(Outcome::Passed)
        }
        Mode::Check => {
            let cur = fs::read(root.join(OUT)).ok();
            if cur.as_deref() != Some(text.as_bytes()) {
                let how = if cur.is_none() { "missing" } else { "stale" };
                writeln!(out, "  FAIL {OUT} is {how}: run {GENERATOR}")?;
                return Ok(Outcome::Failed);
            }
            writeln!(
                out,
                "  ok   {OUT}: {} bytes, every embedded file under a row or baud's own",
                text.len()
            )?;
            Ok(Outcome::Passed)
        }
    }
}

/// **The crates the program links against the register**: the set
/// `Cargo.lock` links on every shipped target is the register's rows, and
/// each row's notice file in the registry, or committed upstream licence file
/// in the tree, still hashes to it; a row whose source the registry does
/// not hold is said and skipped.
fn crate_checks(root: &Path, out: &mut impl Write) -> io::Result<Outcome> {
    let checked = crates::load(root).and_then(|rows| crates::linked(root).map(|l| (rows, l)));
    let (rows, linked) = match checked {
        Ok(x) => x,
        Err(e) => {
            writeln!(out, "  FAIL {e}")?;
            return Ok(Outcome::Failed);
        }
    };
    let mut errs = crates::set_errors(&linked, &rows);
    let n = crates::notice_errors(&rows, &crates::registry_sources());
    errs.extend(n.errors);
    errs.extend(crates::source_errors(root, &rows));
    for s in &n.skipped {
        writeln!(out, "  skip {s}")?;
    }
    for e in &errs {
        writeln!(out, "  FAIL {e}")?;
    }
    Ok(match errs.is_empty() {
        true => Outcome::Passed,
        false => Outcome::Failed,
    })
}

/// `--crates`: the register, written from `Cargo.lock` and the registry.
fn write_crates(root: &Path, out: &mut impl Write) -> io::Result<Outcome> {
    match crates::rows_from_registry(root) {
        Ok(rows) => {
            fs::write(root.join(crates::REGISTER), crates::render(&rows))?;
            writeln!(
                out,
                "  ok   wrote {}, {} crates",
                crates::REGISTER,
                rows.len()
            )?;
            Ok(Outcome::Passed)
        }
        Err(errs) => {
            for e in errs {
                writeln!(out, "  FAIL {e}")?;
            }
            Ok(Outcome::Failed)
        }
    }
}

/// `--list`: each row the binary reaches and the files that reach it, and
/// the includes only tests reach.
fn list(root: &Path, out: &mut impl Write) -> io::Result<Outcome> {
    let placed = third_party::load(root, third_party::SIDECAR).and_then(|side| {
        let rows = rows(&side)?
            .iter()
            .map(|r| str_of(r, "what").unwrap_or_default())
            .collect::<Vec<_>>();
        place(root, &side).map(|p| (p, rows))
    });
    let (p, whats) = match placed {
        Ok(x) => x,
        Err(e) => {
            writeln!(out, "  FAIL {e}")?;
            return Ok(Outcome::Failed);
        }
    };
    for (row, files) in &p.reached {
        let w = whats.get(row.0).map_or("", String::as_str);
        writeln!(
            out,
            "row {}: {}",
            row.0,
            w.chars().take(70).collect::<String>()
        )?;
        for f in files {
            writeln!(out, "    {f}")?;
        }
    }
    writeln!(out, "shipped:")?;
    for f in &p.shipped {
        writeln!(out, "    {f}")?;
    }
    writeln!(out, "test only: {}", p.test.len())?;
    for f in &p.test {
        writeln!(out, "    {f}")?;
    }
    for e in &p.errors {
        writeln!(out, "  FAIL {e}")?;
    }
    Ok(match p.errors.is_empty() {
        true => Outcome::Passed,
        false => Outcome::Failed,
    })
}

/// The repository root a test runs from.
pub fn root_of(manifest: &str) -> PathBuf {
    Path::new(manifest).join("..")
}
