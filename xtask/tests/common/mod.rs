//! A tree that passes `cargo xtask sources` and `third-party --check`
//! with every floor met, and the perturbations that each make one check
//! fire. A module directory, so cargo does not build it as a test target
//! of its own; `sources.rs` and `third_party.rs` drive the cases.
//!
//! Citations are spelled with `format!`, so this file's own text names no
//! key the repository's manifest would have to carry.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::sha256;
use xtask::third_party::{self, Mode, Outcome};

/// A directory under the system's temporary directory, removed on drop.
pub struct Tree(pub PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// How many learn.microsoft.com page entries the manifest holds: the
/// page-text check's floor.
pub const PAGES: usize = 59;
/// How many of them a note cites: the citation floor.
pub const CITED: usize = 50;

pub const DOC: &str = "the cited document\n";
pub const MIT: &str = "MIT License\n\nPermission is hereby granted.\n";
pub const CC_BY: &str = "https://creativecommons.org/licenses/by/4.0/";

pub fn cite(key: &str) -> String {
    format!("[[source: {key}]]")
}

fn page_key(n: usize) -> String {
    format!("ms-page-{n:02}")
}

fn page_entry(n: usize) -> String {
    format!(
        "[sources.{k}]\nkey    = \"{k}\"\ntitle  = \"Page {n}\"\nurl    = \"https://learn.microsoft.com/en-us/x/{n}\"\n\
         file   = \"\"\nsha256 = \"{sha}\"\nbytes  = 50000\nstatus = \"external\"\ngroup  = \"J\"\n\
         revision    = \"{rev}\"\ntext_sha256 = \"{text}\"\ntext_bytes  = 2000\n\n",
        k = page_key(n),
        sha = sha256::hex(format!("page bytes {n}").as_bytes()),
        rev = "a".repeat(40),
        text = sha256::hex(format!("page text {n}").as_bytes()),
    )
}

pub fn manifest() -> String {
    let mut m = String::from("# a manifest\n\n");
    for n in 0..PAGES {
        m.push_str(&page_entry(n));
    }
    m.push_str(&format!(
        "[sources.doc-a]\nkey    = \"doc-a\"\ntitle  = \"Doc A\"\nurl    = \"https://example.org/a.txt\"\n\
         file   = \"a.txt\"\nsha256 = \"{}\"\nbytes  = {}\nstatus = \"retrieved\"\ngroup  = \"C\"\n\n",
        sha256::hex(DOC.as_bytes()),
        DOC.len()
    ));
    m.push_str(
        "[sources.gated-x]\nkey    = \"gated-x\"\ntitle  = \"Never obtained\"\nurl    = \"https://example.org/x\"\n\
         file   = \"\"\nsha256 = \"\"\nstatus = \"gated\"\ngroup  = \"C\"\n",
    );
    m
}

pub fn sidecar() -> String {
    format!(
        "# a sidecar\n\n[text.\"MIT.txt\"]\nlicence = \"MIT\"\nhow     = \"fetched\"\nfrom    = \"https://example.org/mit\"\n\
         sha256  = \"{}\"\n\n[upstream.doc-a]\nname    = \"Doc A\"\nlicence = \"MIT\"\nnotice  = \"LICENSES/MIT.txt\"\n\
         status  = \"kept\"\nfiles   = [\n  \"a.txt\",\n]\n\n\
         [[tree]]\nwhat     = \"A picture under CC BY\"\nlicence  = \"CC-BY-4.0\"\nnotice   = \"in file\"\n\
         upstream = \"https://example.org/pic\"\nstatus   = \"kept\"\npaths    = [\"assets/pic.txt\"]\n\n\
         [[tree]]\nwhat     = \"A transcribed font file\"\nlicence  = \"MIT\"\nnotice   = \"LICENSES/MIT.txt\"\n\
         upstream = \"https://example.org/font\"\nstatus   = \"kept\"\npaths    = [\"baud-font/src/f00.rs\"]\n",
        sha256::hex(MIT.as_bytes())
    )
}

fn licence() -> String {
    let mut l = String::from("baud's licence.\n\n");
    for n in 0..30 {
        l.push_str(&format!(
            "The file assets/p{n:02}.txt is the project's own.\n"
        ));
    }
    l
}

fn provenance() -> String {
    let mut p = String::from("# Provenance\n\n| file | class | notice |\n|---|---|---|\n");
    p.push_str("| `baud-font/src/f00.rs` | a | `LICENSES/MIT.txt` |\n");
    for n in 1..32 {
        p.push_str(&format!("| `baud-font/src/f{n:02}.rs` | b | -- |\n"));
    }
    p
}

fn notes() -> String {
    let mut s = String::from("# Notes\n\n");
    for n in 0..CITED {
        s.push_str(&format!("A claim, {}.\n", cite(&page_key(n))));
    }
    s
}

impl Tree {
    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    pub fn put(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().expect("a parent")).expect("a directory");
        fs::write(p, text).expect("a write");
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).expect("a read")
    }

    /// Replaces the one occurrence of `old` in `rel`.
    pub fn edit(&self, rel: &str, old: &str, new: &str) {
        let s = self.read(rel);
        assert_eq!(s.matches(old).count(), 1, "{rel}: {old:?} must occur once");
        self.put(rel, &s.replacen(old, new, 1));
    }

    pub fn append(&self, rel: &str, text: &str) {
        let s = fs::read_to_string(self.path(rel)).unwrap_or_default();
        self.put(rel, &format!("{s}{text}"));
    }

    pub fn remove(&self, rel: &str) {
        let p = self.path(rel);
        if p.is_dir() {
            fs::remove_dir_all(p).expect("a removal");
        } else {
            fs::remove_file(p).expect("a removal");
        }
    }

    /// THIRD_PARTY.md as the generator writes it for this tree.
    pub fn regenerate(&self) {
        let mut out = Vec::new();
        let o = third_party::run(&self.0, Mode::Write, &mut out).expect("a run");
        assert_eq!(o, Outcome::Passed, "{}", String::from_utf8_lossy(&out));
    }

    pub fn sources(&self) -> (Outcome, String) {
        let mut out = Vec::new();
        let o = xtask::sources::run(&self.0, &mut out).expect("a run");
        (o, String::from_utf8(out).expect("UTF-8"))
    }

    pub fn third_party(&self) -> (Outcome, String) {
        let mut out = Vec::new();
        let o = third_party::run(&self.0, Mode::Check, &mut out).expect("a run");
        (o, String::from_utf8(out).expect("UTF-8"))
    }
}

/// A tree every check passes on.
pub fn base() -> Tree {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("xtask-sources-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    let t = Tree(dir);
    t.put("docs/research/sources/MANIFEST.toml", &manifest());
    t.put("docs/research/sources/a.txt", DOC);
    t.put("docs/research/third-party.toml", &sidecar());
    t.put("docs/notes.md", &notes());
    t.put("LICENSES/MIT.txt", MIT);
    t.put("LICENSE", &licence());
    for n in 0..30 {
        t.put(&format!("assets/p{n:02}.txt"), "own\n");
    }
    t.put(
        "assets/pic.txt",
        &format!("A picture. Licensed under {CC_BY}.\n"),
    );
    t.put("baud-font/PROVENANCE.md", &provenance());
    for n in 0..32 {
        t.put(&format!("baud-font/src/f{n:02}.rs"), "// a font file\n");
    }
    t.regenerate();
    t
}

/// Which gate a case runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    Sources,
    ThirdParty,
}

/// One perturbation: what it does, and a line the gate must then print.
pub struct Case {
    pub name: &'static str,
    pub gate: Gate,
    pub apply: fn(&Tree),
    pub expect: &'static str,
}

const M: &str = "docs/research/sources/MANIFEST.toml";
const S: &str = "docs/research/third-party.toml";

fn with_entry(t: &Tree, extra: &str) {
    t.append(M, &format!("\n{extra}"));
}

/// The perturbations of `cargo xtask sources`' own checks.
pub fn sources_cases() -> Vec<Case> {
    vec![
        Case {
            name: "the manifest is missing",
            gate: Gate::Sources,
            apply: |t| t.remove(M),
            expect: "  FAIL docs/research/sources/MANIFEST.toml is missing",
        },
        Case {
            name: "a title with unescaped quotes",
            gate: Gate::Sources,
            apply: |t| t.edit(M, "title  = \"Doc A\"", "title  = \"Doc \"A\"\""),
            expect: "  FAIL docs/research/sources/MANIFEST.toml is not valid TOML: \
                     unexpected key or value, expected newline, `#` (at line 772, column 16)",
        },
        Case {
            name: "an inline table across lines",
            gate: Gate::Sources,
            apply: |t| with_entry(t, "[sources.z]\nkey = \"z\"\nmeta = { a = 1,\n b = 2 }\n"),
            expect: "is not valid TOML: an inline table spans lines",
        },
        Case {
            name: "a comment in an inline table",
            gate: Gate::Sources,
            apply: |t| with_entry(t, "[sources.z]\nkey = \"z\"\nmeta = { # c\n a = 1 }\n"),
            expect: "is not valid TOML: a comment inside an inline table",
        },
        Case {
            name: "a trailing comma in an inline table",
            gate: Gate::Sources,
            apply: |t| with_entry(t, "[sources.z]\nkey = \"z\"\nmeta = { a = 1, }\n"),
            expect: "is not valid TOML: a comma after an inline table's last pair",
        },
        Case {
            name: "the escape \\e",
            gate: Gate::Sources,
            apply: |t| t.edit(M, "title  = \"Doc A\"", "title  = \"Doc \\e A\""),
            expect: "is not valid TOML: the escape \\e",
        },
        Case {
            name: "the escape \\x",
            gate: Gate::Sources,
            apply: |t| t.edit(M, "title  = \"Doc A\"", "title  = \"\"\"Doc \\x41\"\"\""),
            expect: "is not valid TOML: the escape \\x",
        },
        Case {
            name: "a time with no seconds",
            gate: Gate::Sources,
            apply: |t| with_entry(t, "[sources.z]\nkey = \"z\"\nread = 1979-05-27T07:32Z\n"),
            expect: "is not valid TOML: a time with no seconds",
        },
        Case {
            name: "a byte order mark",
            gate: Gate::Sources,
            apply: |t| t.put(M, &format!("\u{feff}{}", t.read(M))),
            expect: "  FAIL docs/research/sources/MANIFEST.toml is not valid TOML: \
                     a byte order mark, which TOML 1.0 does not allow (at line 1, column 1)",
        },
        Case {
            name: "a header only the line reader sees",
            gate: Gate::Sources,
            apply: |t| {
                t.edit(
                    M,
                    "title  = \"Doc A\"",
                    "title  = \"\"\"Doc A\n[sources.ghost]\n\"\"\"",
                )
            },
            expect: "  FAIL the awk and a TOML parser read different [sources.*] keys:",
        },
        Case {
            name: "a retrieved file is missing",
            gate: Gate::Sources,
            apply: |t| t.remove("docs/research/sources/a.txt"),
            expect: "  FAIL doc-a: status=retrieved but docs/research/sources/a.txt is missing",
        },
        Case {
            name: "a retrieved file changed",
            gate: Gate::Sources,
            apply: |t| t.put("docs/research/sources/a.txt", "another document\n"),
            expect: "  FAIL doc-a: a.txt hashes ",
        },
        Case {
            name: "a sha256 that is not hex",
            gate: Gate::Sources,
            apply: |t| {
                let sha = sha256::hex(format!("page bytes {}", 3).as_bytes());
                t.edit(M, &format!("sha256 = \"{sha}\""), "sha256 = \"XYZ\"");
            },
            expect: "  FAIL ms-page-03: sha256 is not lowercase hex: 'XYZ'",
        },
        Case {
            name: "a sha256 of the wrong length",
            gate: Gate::Sources,
            apply: |t| {
                let sha = sha256::hex(format!("page bytes {}", 3).as_bytes());
                t.edit(
                    M,
                    &format!("sha256 = \"{sha}\""),
                    "sha256 = \"9c07f2a0a0a0a0a0\"",
                );
            },
            expect: "  FAIL ms-page-03: sha256 is 16 chars, want 64",
        },
        Case {
            name: "a retrieved entry with no file",
            gate: Gate::Sources,
            apply: |t| t.edit(M, "file   = \"a.txt\"", "file   = \"\""),
            expect: "  FAIL a.txt is in docs/research/sources/ with no manifest entry",
        },
        Case {
            name: "a file with no entry",
            gate: Gate::Sources,
            apply: |t| t.put("docs/research/sources/deep/b.txt", "b\n"),
            expect: "  FAIL deep/b.txt is in docs/research/sources/ with no manifest entry",
        },
        Case {
            name: "a citation of a key the manifest does not carry",
            gate: Gate::Sources,
            apply: |t| t.append("docs/more.md", &cite("no-such-key")),
            expect: concat!(
                "  FAIL a doc or doc comment cites [[",
                "source: no-such-key]], which the manifest does not carry"
            ),
        },
        Case {
            name: "a citation of a gated key",
            gate: Gate::Sources,
            apply: |t| t.append("src/lib.rs", &format!("//! {}\n", cite("gated-x"))),
            expect: concat!(
                "  FAIL a doc or doc comment cites [[",
                "source: gated-x]], recorded as gated \u{2014} never read"
            ),
        },
        Case {
            name: "too few citations to have scanned anything",
            gate: Gate::Sources,
            apply: |t| t.put("docs/notes.md", &cite("ms-page-00")),
            expect: "  FAIL only 1 citations found; this gate scans the whole tree and should see far more",
        },
        Case {
            name: "the page scope stopped matching",
            gate: Gate::Sources,
            apply: |t| {
                t.edit(
                    M,
                    "https://learn.microsoft.com/en-us/x/58\"",
                    "https://learn.microsoft.com/en-us/x/58.png\"",
                )
            },
            expect: "  FAIL only 58 learn.microsoft.com page entries, want at least 59: the scope stopped matching",
        },
        Case {
            name: "a text_sha256 that is not hex",
            gate: Gate::Sources,
            apply: |t| {
                let h = sha256::hex(format!("page text {}", 7).as_bytes());
                t.edit(
                    M,
                    &format!("text_sha256 = \"{h}\""),
                    "text_sha256 = \"abc\"",
                );
            },
            expect: "  FAIL ms-page-07: text_sha256 is not 64 lowercase hex: 'abc'",
        },
        Case {
            name: "a text_sha256 of the empty string",
            gate: Gate::Sources,
            apply: |t| {
                let h = sha256::hex(format!("page text {}", 7).as_bytes());
                t.edit(
                    M,
                    &format!("text_sha256 = \"{h}\""),
                    &format!("text_sha256 = \"{}\"", sha256::hex(b"")),
                );
            },
            expect: "  FAIL ms-page-07: text_sha256 is the empty string's",
        },
        Case {
            name: "a text_bytes that is a string",
            gate: Gate::Sources,
            apply: |t| {
                let h = sha256::hex(format!("page text {}", 7).as_bytes());
                t.edit(
                    M,
                    &format!("text_sha256 = \"{h}\"\ntext_bytes  = 2000"),
                    &format!("text_sha256 = \"{h}\"\ntext_bytes  = \"2000\""),
                );
            },
            expect: "  FAIL ms-page-07: text_bytes is '2000', want an integer of at least 1024",
        },
        Case {
            name: "a text_bytes under the floor",
            gate: Gate::Sources,
            apply: |t| {
                let h = sha256::hex(format!("page text {}", 7).as_bytes());
                t.edit(
                    M,
                    &format!("text_sha256 = \"{h}\"\ntext_bytes  = 2000"),
                    &format!("text_sha256 = \"{h}\"\ntext_bytes  = 1000"),
                );
            },
            expect: "  FAIL ms-page-07: text_bytes is 1000, want an integer of at least 1024",
        },
        Case {
            name: "a page entry with no text pin",
            gate: Gate::Sources,
            apply: |t| {
                let h = sha256::hex(format!("page text {}", 7).as_bytes());
                t.edit(
                    M,
                    &format!("text_sha256 = \"{h}\"\ntext_bytes  = 2000\n"),
                    "",
                );
            },
            expect: "  FAIL ms-page-07: text_sha256 is not 64 lowercase hex: None",
        },
        Case {
            name: "a revision that is not 40 hex",
            gate: Gate::Sources,
            apply: |t| {
                let h = sha256::hex(format!("page text {}", 7).as_bytes());
                t.edit(
                    M,
                    &format!(
                        "revision    = \"{}\"\ntext_sha256 = \"{h}\"",
                        "a".repeat(40)
                    ),
                    &format!("revision    = \"abc\"\ntext_sha256 = \"{h}\""),
                );
            },
            expect: "  FAIL ms-page-07: revision is not 40 lowercase hex: 'abc'",
        },
        Case {
            name: "an entry out of scope carrying a malformed pin",
            gate: Gate::Sources,
            apply: |t| {
                t.edit(
                    M,
                    "status = \"retrieved\"\n",
                    "status = \"retrieved\"\ntext_sha256 = \"zz\"\n",
                )
            },
            expect: "  FAIL doc-a: text_sha256 is not 64 lowercase hex: 'zz'",
        },
        Case {
            name: "a licence row missing, through the sources gate",
            gate: Gate::Sources,
            apply: |t| t.put("THIRD_PARTY.md", "stale\n"),
            expect: "  FAIL THIRD_PARTY.md is stale: run ",
        },
    ]
}

/// The perturbations of `cargo xtask sources`' `derived_from`, which the
/// Python never had.
pub fn derived_cases() -> Vec<Case> {
    vec![
        Case {
            name: "derived from a key the manifest does not carry",
            gate: Gate::Sources,
            apply: |t| {
                t.edit(
                    M,
                    "group  = \"C\"\n\n[sources.gated-x]",
                    "group  = \"C\"\nderived_from = \"no-such\"\n\n[sources.gated-x]",
                )
            },
            expect: "  FAIL doc-a: derived_from names no-such, which the manifest does not carry",
        },
        Case {
            name: "derived from a gated key",
            gate: Gate::Sources,
            apply: |t| {
                t.edit(M, "group  = \"C\"\n\n[sources.gated-x]", "group  = \"C\"\nderived_from = [\"ms-page-00\", \"gated-x\"]\n\n[sources.gated-x]")
            },
            expect: "  FAIL doc-a: derived_from names gated-x, recorded as gated \u{2014} never read",
        },
        Case {
            name: "derived from itself",
            gate: Gate::Sources,
            apply: |t| {
                t.edit(
                    M,
                    "group  = \"C\"\n\n[sources.gated-x]",
                    "group  = \"C\"\nderived_from = \"doc-a\"\n\n[sources.gated-x]",
                )
            },
            expect: "  FAIL doc-a: derived_from names the entry itself",
        },
        Case {
            name: "derived from a number",
            gate: Gate::Sources,
            apply: |t| {
                t.edit(
                    M,
                    "group  = \"C\"\n\n[sources.gated-x]",
                    "group  = \"C\"\nderived_from = 7\n\n[sources.gated-x]",
                )
            },
            expect: "  FAIL doc-a: derived_from is 7, want a key or a list of keys",
        },
    ]
}

/// The perturbations of the eight third-party checks.
pub fn third_party_cases() -> Vec<Case> {
    vec![
        Case {
            name: "a retrieved file in two groups",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.append(S, "\n[upstream.again]\nname = \"Again\"\nlicence = \"MIT\"\nnotice = \"none\"\nstatus = \"kept\"\nfiles = [\"a.txt\"]\n")
            },
            expect: "  FAIL a.txt is in two groups, doc-a and again",
        },
        Case {
            name: "a group naming a file the manifest does not hold",
            gate: Gate::ThirdParty,
            apply: |t| t.edit(S, "  \"a.txt\",\n", "  \"a.txt\",\n  \"zz.txt\",\n"),
            expect: "  FAIL group doc-a names zz.txt, which the manifest does not hold as retrieved",
        },
        Case {
            name: "a retrieved file in no group",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.put("docs/research/sources/b.txt", DOC);
                with_entry(
                    t,
                    &format!(
                        "[sources.doc-b]\nkey    = \"doc-b\"\nurl    = \"https://example.org/b.txt\"\nfile   = \"b.txt\"\n\
                     sha256 = \"{}\"\nstatus = \"retrieved\"\n",
                        sha256::hex(DOC.as_bytes())
                    ),
                );
            },
            expect: "  FAIL docs/research/sources/b.txt is retrieved in the manifest and in no group of docs/research/third-party.toml",
        },
        Case {
            name: "a licence text with no entry",
            gate: Gate::ThirdParty,
            apply: |t| t.put("LICENSES/extra.txt", "x\n"),
            expect: "  FAIL LICENSES/extra.txt has no [text] entry in docs/research/third-party.toml",
        },
        Case {
            name: "an entry whose text is not there",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.append(S, "\n[text.\"gone.txt\"]\nlicence = \"X\"\nhow = \"fetched\"\nfrom = \"u\"\nsha256 = \"00\"\n")
            },
            expect: "  FAIL [text] gone.txt names LICENSES/gone.txt, which does not exist",
        },
        Case {
            name: "a licence text that drifted",
            gate: Gate::ThirdParty,
            apply: |t| t.put("LICENSES/MIT.txt", "MIT License, edited\n"),
            expect: "  FAIL LICENSES/MIT.txt hashes ",
        },
        Case {
            name: "a licence text nothing cites",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.put("LICENSES/lone.txt", "lone\n");
                t.append(S, &format!("\n[text.\"lone.txt\"]\nlicence = \"X\"\nhow = \"fetched\"\nfrom = \"u\"\nsha256 = \"{}\"\n", sha256::hex(b"lone\n")));
            },
            expect: "  FAIL LICENSES/lone.txt is cited by no group and no [[tree]] entry",
        },
        Case {
            name: "a needs naming no text",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.edit(
                    S,
                    "how     = \"fetched\"\n",
                    "how     = \"fetched\"\nneeds   = [\"LICENSES/nope.txt\"]\n",
                )
            },
            expect: "  FAIL [text] MIT.txt needs LICENSES/nope.txt, which has no [text] entry",
        },
        Case {
            name: "a group's notice that is not there",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.edit(
                    S,
                    "notice  = \"LICENSES/MIT.txt\"",
                    "notice  = \"docs/NOTICE-A\"",
                )
            },
            expect: "  FAIL group doc-a: notice docs/NOTICE-A does not exist",
        },
        Case {
            name: "a tree path that is not there",
            gate: Gate::ThirdParty,
            apply: |t| t.remove("assets/pic.txt"),
            expect: "  FAIL [[tree]] 0 (A picture under CC BY...): assets/pic.txt does not exist",
        },
        Case {
            name: "a tree notice that is not there",
            gate: Gate::ThirdParty,
            apply: |t| t.edit(S, "notice   = \"in file\"", "notice   = \"docs/NOTICE-P\""),
            expect: "  FAIL [[tree]] 0: notice docs/NOTICE-P does not exist",
        },
        Case {
            name: "a group with a status the sidecar does not define",
            gate: Gate::ThirdParty,
            apply: |t| t.edit(S, "status  = \"kept\"\nfiles", "status  = \"fine\"\nfiles"),
            expect: "  FAIL group doc-a: status 'fine' is not one of ['copyleft', 'kept', 'nc', 'no-grant', 'open', 'own', 'unverified']",
        },
        Case {
            name: "a tree entry with a status the sidecar does not define",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.edit(
                    S,
                    "upstream = \"https://example.org/pic\"\nstatus   = \"kept\"",
                    "upstream = \"https://example.org/pic\"\nstatus   = \"fine\"",
                )
            },
            expect: "  FAIL [[tree]] 0: status 'fine' is not one of ['copyleft', 'kept', 'nc', 'no-grant', 'open', 'own', 'unverified']",
        },
        Case {
            name: "a path in LICENSE that is not there",
            gate: Gate::ThirdParty,
            apply: |t| t.append("LICENSE", "See assets/gone.txt.\n"),
            expect: "  FAIL LICENSE:33 names assets/gone.txt, which does not exist",
        },
        Case {
            name: "LICENSE naming too few paths",
            gate: Gate::ThirdParty,
            apply: |t| t.put("LICENSE", "The file assets/p00.txt.\n"),
            expect: "  FAIL LICENSE names 1 repository paths, fewer than 30: the match in licence_paths has stopped working, or the floor is stale",
        },
        Case {
            name: "an in-file notice without its URI",
            gate: Gate::ThirdParty,
            apply: |t| t.put("assets/pic.txt", "A picture.\n"),
            expect: "  FAIL [[tree]] 0: assets/pic.txt is CC-BY-4.0 with its notice in the file, and does not name https://creativecommons.org/licenses/by/4.0/",
        },
        Case {
            name: "no provenance table",
            gate: Gate::ThirdParty,
            apply: |t| t.remove("baud-font/PROVENANCE.md"),
            expect: "  FAIL baud-font/PROVENANCE.md is missing, so no transcribed file can be checked",
        },
        Case {
            name: "a provenance row for a file that is not there",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.append(
                    "baud-font/PROVENANCE.md",
                    "| `baud-font/src/gone.rs` | b | -- |\n",
                )
            },
            expect: "  FAIL baud-font/PROVENANCE.md:37 classes baud-font/src/gone.rs, which does not exist",
        },
        Case {
            name: "a file classed twice",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.append(
                    "baud-font/PROVENANCE.md",
                    "| `baud-font/src/f05.rs` | b | -- |\n",
                )
            },
            expect: "  FAIL baud-font/PROVENANCE.md:37 classes baud-font/src/f05.rs a second time",
        },
        Case {
            name: "a class the table does not define",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.edit(
                    "baud-font/PROVENANCE.md",
                    "| `baud-font/src/f05.rs` | b |",
                    "| `baud-font/src/f05.rs` | d |",
                )
            },
            expect: "  FAIL baud-font/PROVENANCE.md:10 gives baud-font/src/f05.rs class 'd', not a, b or c",
        },
        Case {
            name: "a (b) row naming a notice",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.edit(
                    "baud-font/PROVENANCE.md",
                    "| `baud-font/src/f05.rs` | b | -- |",
                    "| `baud-font/src/f05.rs` | b | `LICENSES/MIT.txt` |",
                )
            },
            expect: "  FAIL baud-font/PROVENANCE.md:10: baud-font/src/f05.rs is (b) and names a notice, `LICENSES/MIT.txt`",
        },
        Case {
            name: "an (a) row naming no text",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.edit(
                    "baud-font/PROVENANCE.md",
                    "| `baud-font/src/f05.rs` | b | -- |",
                    "| `baud-font/src/f05.rs` | a | upstream |",
                )
            },
            expect: "  FAIL baud-font/PROVENANCE.md:10: baud-font/src/f05.rs is (a) and names no text in LICENSES/",
        },
        Case {
            name: "an (a) row no tree entry lists",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.edit(
                    "baud-font/PROVENANCE.md",
                    "| `baud-font/src/f05.rs` | b | -- |",
                    "| `baud-font/src/f05.rs` | a | `LICENSES/MIT.txt` |",
                )
            },
            expect: "  FAIL baud-font/PROVENANCE.md:10: baud-font/src/f05.rs is transcribed (a), and no [[tree]] entry of docs/research/third-party.toml lists it under LICENSES/MIT.txt",
        },
        Case {
            name: "a provenance table under its floor",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.put(
                    "baud-font/PROVENANCE.md",
                    "| `baud-font/src/f00.rs` | a | `LICENSES/MIT.txt` |\n",
                );
                for n in 1..32 {
                    t.remove(&format!("baud-font/src/f{n:02}.rs"));
                }
            },
            expect: "  FAIL baud-font/PROVENANCE.md classes 1 files, fewer than 32: the row pattern has stopped matching, or the floor is stale",
        },
        Case {
            name: "a font file with no class",
            gate: Gate::ThirdParty,
            apply: |t| t.put("baud-font/src/new.rs", "// new\n"),
            expect: "  FAIL baud-font/src/new.rs has no class in baud-font/PROVENANCE.md: read it against what it cites and add its row",
        },
        Case {
            name: "THIRD_PARTY.md stale",
            gate: Gate::ThirdParty,
            apply: |t| {
                t.edit(
                    "THIRD_PARTY.md",
                    "# Third-party files in baud",
                    "# Third-party files",
                )
            },
            expect: "  FAIL THIRD_PARTY.md is stale: run ",
        },
        Case {
            name: "THIRD_PARTY.md missing",
            gate: Gate::ThirdParty,
            apply: |t| t.remove("THIRD_PARTY.md"),
            expect: "  FAIL THIRD_PARTY.md is missing: run ",
        },
        Case {
            name: "the sidecar missing",
            gate: Gate::ThirdParty,
            apply: |t| t.remove(S),
            expect: "  FAIL docs/research/third-party.toml is missing, so no third-party file can be checked",
        },
        Case {
            name: "the sidecar with a byte order mark",
            gate: Gate::ThirdParty,
            apply: |t| t.put(S, &format!("\u{feff}{}", t.read(S))),
            expect: "  FAIL docs/research/third-party.toml: a byte order mark, \
                     which TOML 1.0 does not allow (at line 1, column 1)",
        },
        Case {
            name: "LICENSES/ missing",
            gate: Gate::ThirdParty,
            apply: |t| t.remove("LICENSES"),
            expect: "  FAIL LICENSES is missing, so no third-party file can be checked",
        },
    ]
}

/// Runs a case's gate on a fresh perturbed tree.
pub fn run(case: &Case) -> (Outcome, String) {
    let t = base();
    (case.apply)(&t);
    match case.gate {
        Gate::Sources => t.sources(),
        Gate::ThirdParty => t.third_party(),
    }
}

/// The case's tree, kept for a caller that runs another arm on it.
pub fn tree(case: &Case) -> Tree {
    let t = base();
    (case.apply)(&t);
    t
}

pub fn root(t: &Tree) -> &Path {
    &t.0
}
