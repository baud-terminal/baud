//! **The suite's binary archive holds every pipeline baud builds**
//! (`src/metal/suite.rs`): a pipeline the archive lacks would be refused in
//! every test that draws it, so each is named here before any test meets it.
//!
//! **A Mac only**, like every test in this crate.
#![cfg(target_os = "macos")]

use baud_present_macos::metal::{Pipe, suite_misses};

/// **Every [`Pipe`] builds from the archive, a miss refused**, and one that
/// does not is named.
#[test]
fn every_pipeline_builds_from_the_suite_archive() {
    let missed = suite_misses();
    assert!(
        missed.is_empty(),
        "the suite's archive lacks {missed:?}: a test drawing one is refused"
    );
}

/// **Every entry point `shaders.metal` defines is some [`Pipe`]'s**, so a
/// shader added without a pipeline in the list, which the archive is written
/// from, is named here rather than built some other way.
#[test]
fn every_shader_entry_point_belongs_to_a_pipeline() {
    let shaders = include_str!("../src/shaders.metal");
    let entries: Vec<&str> = shaders
        .lines()
        .filter_map(|line| {
            let rest = line
                .strip_prefix("vertex ")
                .or_else(|| line.strip_prefix("fragment "))?;
            // `<return type> <name>(`
            let (_, after) = rest.split_once(' ')?;
            let (name, _) = after.split_once('(')?;
            Some(name.trim())
        })
        .collect();
    // A positive control: the parse finds the five the app builds first.
    for known in [
        "bg_vertex",
        "glyph_fragment",
        "cell_shape_vertex",
        "flat_fragment",
    ] {
        assert!(
            entries.contains(&known),
            "the parse missed {known}: {entries:?}"
        );
    }
    let named: Vec<&str> = Pipe::ALL
        .iter()
        .flat_map(|p| {
            let (v, f) = p.functions();
            [v, f]
        })
        .collect();
    let orphans: Vec<&&str> = entries.iter().filter(|e| !named.contains(e)).collect();
    assert!(
        orphans.is_empty(),
        "entry points no Pipe names, so the suite's archive leaves them out: {orphans:?}"
    );
    // And the other way: every function a Pipe names is defined.
    let undefined: Vec<&&str> = named.iter().filter(|n| !entries.contains(n)).collect();
    assert!(
        undefined.is_empty(),
        "a Pipe names functions the shaders lack: {undefined:?}"
    );
}

/// **No build that ships has the archive**: `suite-archive` is turned on
/// only in a `dev-dependencies` section, which a build of the app or of a
/// published crate does not read, and declared only in this crate's
/// `[features]`. Every member's manifest is read.
///
/// A build that also builds tests in the same invocation -- `cargo build
/// --all-targets`, `cargo test` -- unifies the dev-dependencies' features
/// (resolver 2), so the app binary it leaves carries the archive; that is
/// a test's binary, not one that ships. The app is shipped from
/// `cargo build --release -p baud` (`scripts/bundle-macos.sh`), which builds
/// no test and reads no dev-dependency.
#[test]
fn only_dev_dependencies_turn_the_suite_archive_on() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace");
    let mut read = 0;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).expect("the workspace").flatten() {
        let manifest = entry.path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        read += 1;
        let member = entry.file_name().to_string_lossy().into_owned();
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                section = line.to_string();
                continue;
            }
            if line.starts_with('#') || !line.contains("suite-archive") {
                continue;
            }
            let declared = member == "baud-present-macos"
                && section == "[features]"
                && line.starts_with("suite-archive =");
            if declared {
                continue;
            }
            found.push((member.clone(), section.clone(), line.to_string()));
        }
    }
    // A positive control: the members that turn it on were read.
    assert!(
        read > 10,
        "only {read} manifests read under {}",
        root.display()
    );
    for member in ["baud-present-macos", "baud-app", "baud-bench"] {
        assert!(
            found.iter().any(|(m, _, _)| m == member),
            "{member} turns the suite archive on for its tests: {found:?}"
        );
    }
    let shipped: Vec<_> = found
        .iter()
        .filter(|(_, section, _)| !section.contains("dev-dependencies"))
        .collect();
    assert!(
        shipped.is_empty(),
        "the suite archive turned on outside dev-dependencies, so a shipped build has it: \
         {shipped:?}"
    );
}

/// **Every process names the same archive**: the key holds nothing that
/// differs between two processes on one machine, such as an object's
/// address, or each test process would write an archive of its own through
/// the shader cache the archive exists to avoid.
#[test]
fn every_process_names_the_same_archive() {
    const CHILD: &str = "BAUD_SUITE_ARCHIVE_PATH_CHILD";
    let here = baud_present_macos::metal::SuiteBuilds::new().archive_path();
    if std::env::var_os(CHILD).is_some() {
        println!("archive={}", here.display());
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().expect("this binary"))
        .args([
            "--exact",
            "every_process_names_the_same_archive",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .expect("the child runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let there = text
        .lines()
        .find_map(|l| l.strip_prefix("archive="))
        .unwrap_or_else(|| panic!("the child named no archive: {text}"));
    assert_eq!(
        std::path::Path::new(there),
        here,
        "two processes, two archives"
    );
}

/// **A spoiled archive under the right key is written again, and the
/// tests go on**: one cut short, and one that loads but holds none of
/// baud's pipelines,
/// each put where the key names, in a folder of this test's own so no other
/// test process reads it. A child loads through each, as a test does.
#[test]
fn a_spoiled_archive_under_its_key_is_written_again() {
    const CHILD: &str = "BAUD_SUITE_ARCHIVE_SPOILED";
    const DIR: &str = "BAUD_SUITE_ARCHIVE_DIR";
    if let Ok(how) = std::env::var(CHILD) {
        spoil_and_load(&how);
        return;
    }
    // The real archive exists first, so a child can cut a copy of it short.
    let real = baud_present_macos::metal::SuiteBuilds::new().archive_path();
    assert!(suite_misses().is_empty(), "the real archive is whole");
    for how in ["cut", "other"] {
        let dir = std::env::temp_dir().join(format!(
            "baud-suite-archive-spoiled-{how}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test's archive folder");
        let out = std::process::Command::new(std::env::current_exe().expect("this binary"))
            .args([
                "--exact",
                "a_spoiled_archive_under_its_key_is_written_again",
                "--nocapture",
            ])
            .env(CHILD, how)
            .env(DIR, &dir)
            .env("BAUD_SUITE_ARCHIVE_REAL", &real)
            .output()
            .expect("the child runs");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            out.status.success(),
            "the {how} archive under its key: {text}"
        );
        assert!(
            text.contains("rewritten"),
            "the child did not report the rewrite: {text}"
        );
    }
}

/// In a child: the archive the key names spoiled `how`, then every pipeline
/// built through it.
fn spoil_and_load(how: &str) {
    use objc2_foundation::{NSString, NSURL};
    use objc2_metal::{
        MTLBinaryArchive, MTLBinaryArchiveDescriptor, MTLDevice, MTLLibrary, MTLPixelFormat,
        MTLRenderPipelineDescriptor,
    };
    let builds = baud_present_macos::metal::SuiteBuilds::new();
    let path = builds.archive_path();
    let real = std::fs::read(std::env::var("BAUD_SUITE_ARCHIVE_REAL").expect("the real archive"))
        .expect("the real archive's bytes");
    match how {
        "cut" => std::fs::write(&path, &real[..real.len() / 2]).expect("a cut archive"),
        // A whole archive, of a pipeline baud never builds: the glyph pass
        // into another pixel format. It loads, and holds none of baud's.
        "other" => {
            let device = objc2_metal::MTLCreateSystemDefaultDevice().expect("a device");
            let library = device
                .newLibraryWithData_error(&dispatch2::DispatchData::from_static_bytes(
                    include_bytes!("../src/baud.metallib"),
                ))
                .expect("baud.metallib");
            let desc = MTLRenderPipelineDescriptor::new();
            let function = |name: &str| {
                library
                    .newFunctionWithName(&NSString::from_str(name))
                    .expect("a function")
            };
            desc.setVertexFunction(Some(&function("glyph_vertex")));
            desc.setFragmentFunction(Some(&function("glyph_fragment")));
            // SAFETY: attachment 0 exists on every pipeline descriptor.
            let att = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
            att.setPixelFormat(MTLPixelFormat::RGBA16Float);
            let other = device
                .newBinaryArchiveWithDescriptor_error(&MTLBinaryArchiveDescriptor::new())
                .expect("an archive");
            other
                .addRenderPipelineFunctionsWithDescriptor_error(&desc)
                .expect("another pipeline in it");
            other
                .serializeToURL_error(&NSURL::fileURLWithPath(&NSString::from_str(
                    &path.display().to_string(),
                )))
                .expect("another archive written");
        }
        other => panic!("no spoiling {other}"),
    }
    let spoiled = std::fs::metadata(&path).expect("the spoiled archive").len();
    let missed = suite_misses();
    assert!(missed.is_empty(), "through the {how} archive: {missed:?}");
    let now = std::fs::metadata(&path).expect("the archive").len();
    assert_ne!(now, spoiled, "the {how} archive was left in place");
    println!("rewritten: {spoiled} bytes to {now}");
}

/// **The app never reads an archive and never refuses a miss.** Built with
/// fail-on-miss, a GPU or OS build the archive was not made for would draw
/// nothing, so the archive's calls live only in `src/metal/suite.rs`, which
/// only `suite-archive` compiles, and the app's own build compiles each
/// pipeline as before. Every Rust file of this crate and of the app is read.
#[test]
fn only_the_suite_module_reads_an_archive_or_refuses_a_miss() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [crates.join("src"), crates.join("../baud-app/src")];
    let calls = [
        "FailOnBinaryArchiveMiss",
        "setBinaryArchives",
        "newBinaryArchiveWithDescriptor",
        "MTLBinaryArchive",
    ];
    let mut read = 0;
    let mut found = Vec::new();
    let mut stack: Vec<std::path::PathBuf> = roots.to_vec();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("a source folder").flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                read += 1;
                let text = std::fs::read_to_string(&p).expect("a source file");
                if p.ends_with("src/metal/suite.rs") {
                    continue;
                }
                for call in calls {
                    if text.contains(call) {
                        found.push(format!("{}: {call}", p.display()));
                    }
                }
            }
        }
    }
    assert!(read > 100, "only {read} source files read");
    assert!(
        found.is_empty(),
        "archive calls outside src/metal/suite.rs: {found:?}"
    );
    let metal = std::fs::read_to_string(crates.join("src/metal.rs")).expect("metal.rs");
    for gate in [
        "#[cfg(feature = \"suite-archive\")]\nmod suite;",
        "#[cfg(not(feature = \"suite-archive\"))]\n    let built = device.newRenderPipelineStateWithDescriptor_error(&desc);",
    ] {
        assert!(metal.contains(gate), "metal.rs lost its gate: {gate}");
    }
}
