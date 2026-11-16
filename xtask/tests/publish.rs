//! Every check of `cargo xtask publish` fires, each on one perturbation of
//! a workspace that passes, and the three exit codes are what the Release
//! job reads.

mod publish_tree;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use publish_tree::{Tree, flags, put, release};
use xtask::publish::{self, Bytes, Exit, Flags, Folder, LIMIT, Pack, Writes};

/// The perturbation fails the run with each of `lines` among its output.
fn fires_all(t: &Tree, lines: &[&str]) {
    let (exit, out) = t.run(flags());
    assert_eq!(exit, Exit::Failed, "the check passed:\n{out}");
    for line in lines {
        assert!(
            out.lines().any(|l| l == *line),
            "no line {line:?} in:\n{out}"
        );
    }
}

fn fires(t: &Tree, line: &str) {
    fires_all(t, &[line]);
}

#[test]
fn the_base_tree_passes_and_exits_0_under_release() {
    let t = Tree::base();
    assert_eq!(
        t.run(flags()),
        (
            Exit::Passed,
            "checked: 10 crates to publish, 5 not\nok\n".to_string()
        )
    );
    assert_eq!(t.run(release()).0, Exit::Passed);
}

#[test]
fn a_notice_is_what_the_rows_give() {
    let t = Tree::base();
    assert_eq!(
        t.read("c1/NOTICE"),
        "Third-party material in the c1 crate\n\n\
         The licence of c1 is LICENSE. This file lists the material from other\n\
         projects that files in this crate carry: for each, the files by their paths\n\
         in this crate, the licence it keeps, and where the notice that licence asks\n\
         for is. It is generated from baud's record of third-party files, in its\n\
         repository at https://example.org/baud, whose THIRD_PARTY.md lists every\n\
         such file there.\n\n\
         A table copied from an upstream project.\n\
         \x20 Files:   vendored.txt\n\
         \x20 Holder:  The Example Authors\n\
         \x20 Licence: MIT\n\
         \x20 Notice:  LICENSES/MIT.txt\n\
         \x20 Status:  kept: redistributable, and its notice is satisfied\n\
         \x20 From:    https://example.org/vendored\n"
    );
    assert!(t.read("c2/NOTICE").ends_with(
        "\n\nNo file in this crate carries material from another project that baud's\n\
         record of third-party files lists.\n"
    ));
}

#[test]
fn every_notice_is_written_back_byte_equal() {
    let t = Tree::base();
    let before: Vec<String> = ["baud", "c1", "c2"]
        .iter()
        .map(|n| t.read(&format!("{n}/NOTICE")))
        .collect();
    for n in ["baud", "c1", "c2"] {
        t.put(&format!("{n}/NOTICE"), "");
    }
    fires(
        &t,
        "FAIL c1: NOTICE is not what its files' rows give: run cargo xtask publish --write-notices",
    );
    let written = Flags {
        notices: Writes::Yes,
        ..flags()
    };
    assert_eq!(t.run(written).0, Exit::Passed);
    let after: Vec<String> = ["baud", "c1", "c2"]
        .iter()
        .map(|n| t.read(&format!("{n}/NOTICE")))
        .collect();
    assert_eq!(before, after);
}

#[test]
fn a_crate_baud_reaches_that_says_publish_false() {
    let t = Tree::base();
    t.edit("c8/Cargo.toml", "readme = ", "publish = false\nreadme = ");
    fires(
        &t,
        "FAIL c8: `baud` depends on it, so it must be published, and it says publish = false",
    );
}

#[test]
fn a_published_crate_baud_does_not_reach() {
    let t = Tree::base();
    t.edit(
        "baud/Cargo.toml",
        "c7 = { path = \"../c7\", version = \"0.1.0\" }\n",
        "",
    );
    fires_all(
        &t,
        &[
            "FAIL c7: published, and `baud` does not depend on it",
            "FAIL unpublished members are ['baud-bench', 'baud-difftest', 'baud-present-vulkan', \
             'baud-sfte-difftest', 'c7', 'xtask'], UNPUBLISHED says ['baud-bench', \
             'baud-difftest', 'baud-present-vulkan', 'baud-sfte-difftest', 'xtask']",
            "FAIL only 9 crates reached from `baud`: the dependency walk has stopped working",
        ],
    );
}

#[test]
fn a_bad_keyword() {
    let t = Tree::base();
    t.edit(
        "c2/Cargo.toml",
        "[\"terminal\"]",
        "[\"terminal\", \"9lives\"]",
    );
    fires(
        &t,
        "FAIL c2: keywords ['terminal', '9lives'] are not one to five crates.io keywords",
    );
}

/// Python's `$` matches before a final newline, so a keyword that ends in
/// one passed there, and passes here.
#[test]
fn a_keyword_with_a_final_newline_passes_as_it_did() {
    let t = Tree::base();
    t.edit("c2/Cargo.toml", "[\"terminal\"]", "[\"terminal\\n\"]");
    assert_eq!(t.run(flags()).0, Exit::Passed);
    t.edit("c2/Cargo.toml", "[\"terminal\\n\"]", "[\"terminal\\n\\n\"]");
    fires(
        &t,
        "FAIL c2: keywords ['terminal\\n\\n'] are not one to five crates.io keywords",
    );
}

#[test]
fn a_bad_category_and_a_missing_field() {
    let t = Tree::base();
    t.edit(
        "c3/Cargo.toml",
        "\"command-line-utilities\"",
        "\"terminals\"",
    );
    t.edit("c3/Cargo.toml", "description = \"The c3 crate.\"\n", "");
    t.edit(
        "c4/Cargo.toml",
        "readme = \"README.md\"",
        "readme = \"READ.md\"",
    );
    fires_all(
        &t,
        &[
            "FAIL c3: categories ['terminals'] are not one to five of CATEGORIES",
            "FAIL c3: no description",
            "FAIL c4: readme 'READ.md' is not a file",
        ],
    );
}

#[test]
fn a_licence_expression_drift() {
    let t = Tree::base();
    t.edit(
        "c1/Cargo.toml",
        "\"CC-BY-NC-SA-4.0 AND MIT\"",
        "\"CC-BY-NC-SA-4.0\"",
    );
    fires(
        &t,
        "FAIL c1: license is 'CC-BY-NC-SA-4.0', and its files give 'CC-BY-NC-SA-4.0 AND MIT': \
         run cargo xtask publish --write-licences",
    );
    let written = Flags {
        licences: Writes::Yes,
        ..flags()
    };
    assert_eq!(t.run(written).0, Exit::Passed);
    assert!(
        t.read("c1/Cargo.toml")
            .contains("license = \"CC-BY-NC-SA-4.0 AND MIT\"\n")
    );
}

#[test]
fn a_sidecar_spdx_crates_io_refuses_and_a_licence_with_none() {
    let t = Tree::base();
    t.edit(
        "docs/research/third-party.toml",
        "licence = \"MIT\"\nnotice = \"LICENSES/MIT.txt\"\nupstream",
        "licence = \"MIT-ish\"\nnotice = \"LICENSES/MIT.txt\"\nupstream",
    );
    fires(
        &t,
        "FAIL c1: ships vendored.txt (A vendored table...), whose licence 'MIT-ish' is no SPDX \
         expression crates.io takes, and the entry has no spdx field",
    );
    t.edit(
        "docs/research/third-party.toml",
        "status = \"kept\"\npublic",
        "spdx = \"MIT WITH X\"\nstatus = \"kept\"\npublic",
    );
    fires(
        &t,
        "FAIL docs/research/third-party.toml: spdx 'MIT WITH X' is no SPDX expression crates.io takes",
    );
}

#[test]
fn an_unversioned_path_dependency() {
    let t = Tree::base();
    t.edit(
        "baud/Cargo.toml",
        "c2 = { path = \"../c2\", version = \"0.1.0\" }",
        "c2 = { path = \"../c2\" }",
    );
    fires(
        &t,
        "FAIL baud: [dependencies] c2 has version None, not '0.1.0': cargo refuses to package a path dependency with none",
    );
}

#[test]
fn a_licence_that_is_not_the_roots_and_a_notice_left_out() {
    let t = Tree::base();
    t.put("c2/LICENSE", "another licence\n");
    fs::remove_file(t.path("c1/LICENSES/MIT.txt")).expect("the notice");
    fires_all(
        &t,
        &[
            "FAIL c2: its LICENSE is not the root's",
            "FAIL c1: ships vendored.txt (A vendored table) without LICENSES/MIT.txt",
        ],
    );
}

/// A third-party file in a crate with its licence file left out of the crate.
#[test]
fn a_row_whose_notice_file_is_not_in_the_crate() {
    let t = Tree::base();
    t.edit(
        "docs/research/third-party.toml",
        "notice = \"LICENSES/MIT.txt\"\nupstream",
        "notice = \"c1/gone-LICENSE\"\nupstream",
    );
    fires(
        &t,
        "FAIL c1: ships vendored.txt (A vendored table) without its notice c1/gone-LICENSE",
    );
}

/// The same with a notice in prose, which names the text beside the file.
#[test]
fn a_row_whose_notice_in_prose_names_a_file_not_in_the_crate() {
    let t = Tree::base();
    t.edit(
        "docs/research/third-party.toml",
        "notice = \"LICENSES/MIT.txt\"\nupstream",
        "notice = \"the text beside it (Gone-LICENSE.txt)\"\nupstream",
    );
    fires(
        &t,
        "FAIL c1: ships vendored.txt (A vendored table) without its notice c1/Gone-LICENSE.txt",
    );
}

#[test]
fn an_unsettled_row_prints_passes_and_is_refused_at_release() {
    let t = Tree::base();
    t.edit(
        "docs/research/third-party.toml",
        "status = \"kept\"\npublic",
        "status = \"open\"\npublic",
    );
    t.edit(
        "c1/NOTICE",
        "kept: redistributable, and its notice is satisfied",
        "open: an open question, recorded where the file is",
    );
    let (exit, out) = t.run(flags());
    assert_eq!(exit, Exit::Passed, "{out}");
    let line = "c1: ships vendored.txt (A vendored table...), status open";
    assert!(
        out.contains(&format!("\nawaiting the owner: {line}\nok\n")),
        "{out}"
    );
    let (exit, out) = t.run(release());
    assert_eq!(exit, Exit::Refused, "{out}");
    assert!(
        out.ends_with(&format!(
            "REFUSED: {line}\nnot releasable: 1 shipped files' licences await the owner\n"
        )),
        "{out}"
    );
    t.put("c3/LICENSE", "another licence\n");
    assert_eq!(t.run(release()).0, Exit::Failed);
}

#[test]
fn a_notice_that_is_not_the_rows() {
    let t = Tree::base();
    t.edit("c1/NOTICE", "The Example Authors", "Someone Else");
    fires(
        &t,
        "FAIL c1: NOTICE is not what its files' rows give: run cargo xtask publish --write-notices",
    );
    fs::remove_file(t.path("c2/NOTICE")).expect("the NOTICE");
    fires(
        &t,
        "FAIL c2: has no NOTICE: run cargo xtask publish --write-notices",
    );
}

#[test]
fn a_stray_notice_word() {
    let t = Tree::base();
    let side = "docs/research/third-party.toml";
    t.edit(
        side,
        "from an upstream project.",
        "from an upstream project, decided on 2026-10-10 for c2/README.md, as doc-key-007 says.",
    );
    t.edit(side, "holder = \"The Example Authors\"", "holder = \"The Example Authors\"\nfrom = [\"docs/research/sources/doc-005.txt\", \"ftp://x\"]");
    let lines: Vec<String> = [
        "decided",
        "2026-10-10",
        "doc-key-007",
        "c2/README.md",
        "From: ftp://x",
    ]
    .iter()
    .map(|f| format!("FAIL c1: NOTICE says '{f}' (A vendored table...)"))
    .collect();
    fires_all(&t, &lines.iter().map(String::as_str).collect::<Vec<_>>());
    // A source file is named by the URL its manifest entry records.
    let written = Flags {
        notices: Writes::Yes,
        ..flags()
    };
    t.run(written);
    assert!(
        t.read("c1/NOTICE")
            .contains("https://example.org/005 and ftp://x")
    );
}

#[test]
fn a_settled_row_with_no_public_or_holder() {
    let t = Tree::base();
    t.edit(
        "docs/research/third-party.toml",
        "holder = \"The Example Authors\"\n",
        "",
    );
    fires(
        &t,
        "FAIL c1: NOTICE says 'no holder field' (A vendored table...)",
    );
}

#[test]
fn a_root_without_the_workspace_profile() {
    let t = Tree::base();
    t.edit(
        "baud/Cargo.toml",
        "codegen-units = 1\n",
        "codegen-units = 16\n",
    );
    fires(
        &t,
        "FAIL baud: [profile.release] is {'lto': 'fat', 'codegen-units': 16}, not the workspace's \
         {'lto': 'fat', 'codegen-units': 1}",
    );
}

#[test]
fn a_symlink_out_of_the_repository() {
    let t = Tree::base();
    put(&t.top.join("outside.txt"), "outside\n");
    symlink(t.top.join("outside.txt"), t.path("c2/outside.txt")).expect("the link");
    fires(
        &t,
        "FAIL c2: outside.txt links to ../outside.txt, which is not a file in the repository",
    );
}

/// A link to a file in the repository counts as that file: the row of
/// `c1/vendored.txt` reaches `c2`, which then needs its notice.
#[test]
fn a_symlink_in_the_repository_counts_as_its_target() {
    let t = Tree::base();
    symlink("../c1/vendored.txt", t.path("c2/table.txt")).expect("the link");
    fires(
        &t,
        "FAIL c2: ships table.txt (A vendored table) without LICENSES/MIT.txt",
    );
}

/// A workspace of one or two crates, packaged by [`publish::package`].
fn small(t: &Tree, crates: &[(&str, &str, &str)]) {
    let members: Vec<String> = crates.iter().map(|(n, _, _)| format!("\"{n}\"")).collect();
    t.put(
        "Cargo.toml",
        &format!(
            "[workspace]\nmembers = [{}]\nresolver = \"3\"\n",
            members.join(", ")
        ),
    );
    for (n, deps, lib) in crates {
        t.put(
            &format!("{n}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{n}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
                 license = \"MIT\"\ndescription = \"A probe of cargo xtask publish.\"\n{deps}"
            ),
        );
        t.put(&format!("{n}/src/lib.rs"), lib);
    }
}

fn package(t: &Tree, names: &[&str], pack: Pack, folder: Folder) -> publish::Packed {
    publish::package(&t.ws, names, "0.1.0", pack, &t.cargo(), folder).expect("a run")
}

/// The verify's freshness, on two crates, `b` on `a` by
/// path and version, packaged three times with the version never bumped.
/// A build from an earlier step's unpack or compile of `a` would pass the
/// third step and fail the second.
#[test]
fn each_verify_reads_its_own_unpack_and_leaves_none() {
    let (a, b) = ("check-publish-fresh-probe-a", "check-publish-fresh-probe-b");
    let one = "pub fn one() -> u8 {\n    1\n}\n";
    let two = "pub fn two() -> u8 {\n    2\n}\n";
    let steps = [
        (one.to_string(), "one", None),
        (format!("{one}\n{two}"), "two", None),
        (two.to_string(), "one", Some("cannot find function `one`")),
    ];
    let t = Tree::bare();
    let dep = format!("\n[dependencies]\n{a} = {{ path = \"../{a}\", version = \"0.1.0\" }}\n");
    let mut seen = Vec::new();
    for (i, (lib_a, call, fails)) in steps.iter().enumerate() {
        let lib_b = format!(
            "pub fn f() -> u8 {{\n    {}::{call}()\n}}\n",
            a.replace('-', "_")
        );
        small(&t, &[(a, "", lib_a), (b, &dep, &lib_b)]);
        let p = package(&t, &[a, b], Pack::Verify, Folder::Fresh);
        let said = p.errs.join("\n");
        match fails {
            None => assert!(p.errs.is_empty(), "step {}: {said}", i + 1),
            Some(why) => assert!(said.contains(why), "step {}: {said}", i + 1),
        }
        let read: Vec<&Path> = p.read.iter().map(|r| r.as_path()).collect();
        assert_eq!(read.len(), 1, "step {}: {read:?}", i + 1);
        let unpack = read[0];
        assert_eq!(
            unpack.file_name().and_then(|n| n.to_str()),
            Some(format!("{a}-0.1.0").as_str())
        );
        let folder = unpack.parent().expect("its folder").to_path_buf();
        assert!(!seen.contains(&folder), "step {}: {folder:?} again", i + 1);
        assert!(
            !unpack.exists() && !folder.exists(),
            "step {}: left behind",
            i + 1
        );
        seen.push(folder);
    }
}

#[test]
fn a_stale_unpack_folder() {
    let a = "check-publish-stale-probe-a";
    let b = "check-publish-stale-probe-b";
    let t = Tree::bare();
    let dep = format!("\n[dependencies]\n{a} = {{ path = \"../{a}\", version = \"0.1.0\" }}\n");
    small(
        &t,
        &[
            (a, "", "pub fn one() {}\n"),
            (b, &dep, "pub use check_publish_stale_probe_a::one;\n"),
        ],
    );
    let at = t.top.join("target").join("check-publish-fixed");
    let first = package(&t, &[a, b], Pack::Verify, Folder::At(at.clone()));
    assert!(first.errs.is_empty(), "{:?}", first.errs);
    let folder = first
        .read
        .iter()
        .next()
        .expect("an unpack")
        .parent()
        .expect("its folder")
        .to_path_buf();
    // The same build folder is the same registry URL, so the same unpack
    // folder: made before the run, as an earlier run would have left it.
    fs::create_dir_all(&folder).expect("the stale folder");
    let second = package(&t, &[a, b], Pack::Verify, Folder::At(at));
    assert_eq!(
        second.errs,
        vec![format!(
            "cargo built a crate from {}, which was there before this run: \
             what is unpacked there may be an older .crate's",
            folder.display()
        )]
    );
}

#[test]
fn a_crate_that_does_not_build_from_its_crate() {
    let t = Tree::bare();
    t.put("outside.txt", "outside\n");
    small(
        &t,
        &[(
            "probe-leak",
            "",
            "pub const X: &str = include_str!(\"../../outside.txt\");\n",
        )],
    );
    let p = package(&t, &["probe-leak"], Pack::Verify, Folder::Fresh);
    assert!(
        p.errs.len() == 1 && p.errs[0].starts_with("cargo package failed: "),
        "{:?}",
        p.errs
    );
    assert!(p.errs[0].contains("outside.txt"), "{:?}", p.errs);
    // Without the build it packages.
    let p = package(&t, &["probe-leak"], Pack::Size, Folder::Fresh);
    assert!(
        p.errs.is_empty() && p.crates.contains_key("probe-leak"),
        "{:?}",
        p.errs
    );
}

/// Bytes no compressor shrinks: xorshift64.
fn noise(n: u64) -> Vec<u8> {
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x.to_le_bytes()[0]
        })
        .collect()
}

#[test]
fn an_oversize_crate() {
    let t = Tree::bare();
    small(&t, &[("probe-big", "", "")]);
    fs::write(t.path("probe-big/noise.bin"), noise(LIMIT.0 + (1 << 19))).expect("the noise");
    let p = package(&t, &["probe-big"], Pack::Size, Folder::Fresh);
    assert!(p.errs.is_empty(), "{:?}", p.errs);
    let (size, manifest) = &p.crates["probe-big"];
    assert!(*size > LIMIT, "{size:?}");
    assert!(String::from_utf8_lossy(manifest).contains("name = \"probe-big\""));
    let mut out = Vec::new();
    let errs = publish::sized(&p.crates, &toml::Table::new(), &mut out).expect("sized");
    assert_eq!(
        errs,
        vec![format!(
            "probe-big: {} bytes, over crates.io's 10485760",
            size.0
        )]
    );
    assert!(
        String::from_utf8(out)
            .expect("UTF-8")
            .starts_with("  probe-big: 10.5")
    );
}

#[test]
fn the_packaged_root_manifest_is_held_to_the_profile() {
    let ws: toml::Table = "[profile.release]\nlto = \"fat\"\n".parse().expect("toml");
    let crates = BTreeMap::from([(
        "baud".to_string(),
        (Bytes(1), b"[package]\nname = \"baud\"\n".to_vec()),
    )]);
    let mut out = Vec::new();
    assert_eq!(
        publish::sized(&crates, &ws, &mut out).expect("sized"),
        vec!["baud: [profile.release] is None, not the workspace's {'lto': 'fat'} in its .crate"]
    );
    assert_eq!(
        String::from_utf8(out).expect("UTF-8"),
        "  baud: 0.00 MiB compressed\n"
    );
}

/// `{:.2f}` of the size in MiB rounds the exact quotient half to even, as
/// Python's float format does: 0.125 MiB is "0.12" and 0.375 is "0.38".
#[test]
fn sizes_print_as_python_rounds_them() {
    for (b, want) in [
        (131_072, "0.12"),
        (393_216, "0.38"),
        (5_242, "0.00"),
        (5_243, "0.01"),
        (1_048_575, "1.00"),
        (4_582_277, "4.37"),
        (LIMIT.0, "10.00"),
    ] {
        assert_eq!(publish::mib(Bytes(b)), want, "{b}");
    }
}

#[test]
fn spdx_expressions_and_the_licence_field() {
    for ok in [
        "MIT",
        "(MIT OR Apache-2.0)",
        "LicenseRef-X.1",
        "LicenseRef-X\n",
        "MIT AND (Zlib OR X11)",
    ] {
        assert!(publish::spdx_ok(ok), "{ok:?}");
    }
    for bad in [
        "",
        "GPL-3.0",
        "MIT WITH X",
        "MIT AND",
        "(MIT",
        "MIT)",
        "AND MIT",
        "LicenseRef-",
    ] {
        assert!(!publish::spdx_ok(bad), "{bad:?}");
    }
    let terms = [
        "MIT AND Zlib",
        "",
        "MIT OR Apache-2.0",
        "(X11 OR MIT)",
        "CC-BY-NC-SA-4.0",
    ]
    .map(String::from);
    assert_eq!(
        publish::licence_expr(&terms),
        "CC-BY-NC-SA-4.0 AND (MIT OR Apache-2.0) AND (X11 OR MIT) AND MIT AND Zlib"
    );
}

#[test]
fn the_flags_are_read_by_presence() {
    let f = Flags::parse(&["--size", "--release", "--bogus"]);
    assert_eq!((f.pack, f.release), (Pack::Size, publish::Release::Yes));
    assert_eq!(Flags::parse(&["--size", "--verify"]).pack, Pack::Verify);
    assert_eq!(Flags::parse(&[]), flags());
}

/// The profile compares as `tomllib`'s values compare in Python: a table
/// without its order, `True == 1`, and an int equal to a float only when
/// the float is exactly that int.
#[test]
fn the_profile_compares_as_python_compares() {
    let ws: toml::Table = "[profile.release]\nlto = true\nopt-level = 3\nx = 9007199254740993\n"
        .parse()
        .expect("toml");
    let same: toml::Table = "[profile.release]\nx = 9007199254740993\nopt-level = 3.0\nlto = 1\n"
        .parse()
        .expect("toml");
    assert_eq!(
        publish::root_settings("baud", &same, &ws),
        Vec::<String>::new()
    );
    let near: toml::Table =
        "[profile.release]\nlto = true\nopt-level = 3\nx = 9007199254740992.0\n"
            .parse()
            .expect("toml");
    assert_eq!(publish::root_settings("baud", &near, &ws).len(), 1);
}
