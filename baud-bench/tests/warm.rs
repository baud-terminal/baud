//! The warm build (`scripts/warm.sh`) and the guard every measurement runs
//! (`baud_bench::warm`): a measurement refuses beside an unpaused watcher or
//! build and runs once it is paused, every measuring `main` asks, and the
//! watcher builds exactly what `devenv test` builds.

use std::path::{Path, PathBuf};

use baud_bench::warm::{self, Warm};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("baud-bench sits in the repository")
        .to_path_buf()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn an_unpaused_watcher_or_build_refuses_and_a_paused_one_does_not() {
    let listing = "\
  101 watchexec --shell=none --debounce 2s -- bash /r/scripts/warm.sh build RUSTFLAGS='-D warnings' cargo nextest run --release --workspace --no-run
  102 bash /r/scripts/warm.sh build RUSTFLAGS='-D warnings' cargo nextest run --release --workspace --no-run
  103 bash /r/scripts/warm.sh pause
  104 bash /r/scripts/warm.sh.orig build
  105 /usr/bin/vim /r/scripts/warm.sh
  106 bash scripts/warm.sh build cargo
";
    let r = Some(PathBuf::from("/r"));
    let expect = |pid: &str, root: &Option<PathBuf>| Warm {
        pid: pid.to_owned(),
        root: root.clone(),
    };

    // Nothing paused: the watcher and its build, and the relative spelling,
    // whose checkout cannot be read. Not the pause, a look-alike script or an
    // editor.
    assert_eq!(
        warm::unpaused(listing, |_| false),
        [expect("101", &r), expect("102", &r), expect("106", &None)]
    );
    // /r paused: only the one no pause can be read for is left.
    assert_eq!(
        warm::unpaused(listing, |root| root == Path::new("/r")),
        [expect("106", &None)]
    );
    // Another checkout's pause covers nothing here.
    assert_eq!(
        warm::unpaused(listing, |root| root == Path::new("/s")).len(),
        3
    );
    // A machine with no warm process lets a measurement start.
    assert_eq!(
        warm::unpaused("  1 /sbin/launchd\n  2 cargo bench\n", |_| false),
        []
    );
}

/// The live half: `warm.sh build` runs a fake build under the real
/// `scripts/bounded`, in a copy of the scripts in a folder of its own, and
/// the guard is read over this machine's real process list, narrowed to that
/// folder so a watcher elsewhere on the machine changes nothing here. macOS
/// only: the pause's kill is bounded's macOS watcher (`warm.sh` says why).
#[cfg(target_os = "macos")]
#[test]
fn a_live_build_refuses_a_measurement_until_warm_pause_stops_it() {
    use baud_bench::warm::PAUSE;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    struct Scene {
        dir: PathBuf,
        child: Option<Child>,
        sleeper: PathBuf,
    }
    impl Drop for Scene {
        fn drop(&mut self) {
            if let Ok(pid) = std::fs::read_to_string(&self.sleeper) {
                let _ = Command::new("kill").args(["-9", pid.trim()]).status();
            }
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    fn alive(pid: &str) -> bool {
        Command::new("kill")
            .args(["-0", pid])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    let repo = repo();
    let made = std::env::temp_dir().join(format!("baud-warm-{}", std::process::id()));
    std::fs::create_dir_all(made.join("scripts")).unwrap();
    let dir = std::fs::canonicalize(&made).unwrap();
    for file in [
        "scripts/warm.sh",
        "scripts/bounded",
        "scripts/bounded.rs",
        "budgets.toml",
    ] {
        std::fs::copy(repo.join(file), dir.join(file)).unwrap();
    }
    let script = dir.join("scripts/warm.sh");
    let sleeper = dir.join("sleeper.pid");
    let mut scene = Scene {
        dir: dir.clone(),
        child: None,
        sleeper: sleeper.clone(),
    };
    let build = format!("echo $$ >'{}'; exec sleep 600", sleeper.display());
    let child = Command::new("bash")
        .arg(&script)
        .args(["build", &build])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let build_pid = child.id().to_string();
    scene.child = Some(child);

    // bounded compiles its watcher on a first run; then the build starts.
    let until = Instant::now() + Duration::from_secs(120);
    let sleeper_pid = loop {
        let pid = std::fs::read_to_string(&sleeper).unwrap_or_default();
        if !pid.trim().is_empty() {
            break pid.trim().to_owned();
        }
        let ended = scene.child.as_mut().unwrap().try_wait().unwrap();
        assert!(ended.is_none(), "warm.sh build ended first: {ended:?}");
        assert!(Instant::now() < until, "the fake build never started");
        std::thread::sleep(Duration::from_millis(50));
    };

    let here = format!("{}/scripts/warm.sh", dir.display());
    let mine = || -> String {
        let listing = warm::listing().unwrap();
        listing
            .lines()
            .filter(|l| l.contains(&here))
            .map(|l| format!("{l}\n"))
            .collect()
    };
    let paused = |root: &Path| root.join(PAUSE).exists();

    // Red: a build in flight and no pause.
    let refused = warm::unpaused(&mine(), paused);
    assert!(
        refused.contains(&Warm {
            pid: build_pid.clone(),
            root: Some(dir.clone()),
        }),
        "the build in flight was not seen: {refused:?}"
    );

    let pause = Command::new("bash")
        .arg(&script)
        .arg("pause")
        .output()
        .unwrap();
    assert!(pause.status.success(), "{pause:?}");
    assert!(dir.join(PAUSE).exists());

    // The pause stopped the build: the fake is gone and so is warm.sh.
    assert!(!alive(&sleeper_pid), "the fake build survived the pause");
    let until = Instant::now() + Duration::from_secs(30);
    while scene.child.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < until,
            "warm.sh build did not end after the pause"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // Green: paused, a measurement may start.
    assert_eq!(warm::unpaused(&mine(), paused), []);

    // A build started while paused starts nothing.
    let again = Command::new("bash")
        .arg(&script)
        .args(["build", "exit 9"])
        .output()
        .unwrap();
    assert!(again.status.success(), "{again:?}");

    let resume = Command::new("bash")
        .arg(&script)
        .arg("resume")
        .output()
        .unwrap();
    assert!(resume.status.success(), "{resume:?}");
    assert!(!dir.join(PAUSE).exists());
}

/// Every bench is a measurement, and these examples are the ones that time
/// something; each `main` asks before it measures. A new bench that does not
/// fails here.
#[test]
fn every_measuring_main_asks_first() {
    let repo = repo();
    let call = "baud_bench::warm::refuse_while_warm();";
    let mut files: Vec<PathBuf> = std::fs::read_dir(repo.join("baud-bench/benches"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    assert!(files.len() > 10, "{files:?}");
    for example in [
        "key-latency",
        "idle-cpu",
        "startup-bench",
        "shader-cache-bench",
    ] {
        files.push(repo.join(format!("baud-bench/examples/{example}.rs")));
    }
    for file in &files {
        let text = read(file);
        let mains = text.matches("fn main()").count();
        assert!(mains > 0, "{}: no main", file.display());
        assert_eq!(
            text.matches(call).count(),
            mains,
            "{}: every main calls {call}",
            file.display()
        );
    }
    let multibench = read(&repo.join("bench-others/multibench/src/main.rs"));
    assert!(multibench.contains("#[path = \"../../../baud-bench/src/warm.rs\"]"));
    assert_eq!(multibench.matches("warm::refuse_while_warm();").count(), 1);
}

/// The watcher's build is `test:unit`'s: one binding in devenv.nix, used by
/// both, so RUSTFLAGS and the profile cannot differ, and the watcher builds
/// from the root, where `CARGO_TARGET_DIR`'s relative `./target` is the
/// gate's.
#[test]
fn the_watcher_builds_what_the_gate_builds() {
    let repo = repo();
    let nix = read(&repo.join("devenv.nix"));
    let sh = read(&repo.join("scripts/warm.sh"));

    let binding = nix
        .lines()
        .find_map(|l| l.trim().strip_prefix("unitBuild = \""))
        .and_then(|l| l.strip_suffix("\";"))
        .expect("devenv.nix binds unitBuild");
    for word in [
        "RUSTFLAGS='-D warnings'",
        "cargo nextest run",
        "--release",
        "${scope}",
    ] {
        assert!(binding.contains(word), "unitBuild lacks {word}: {binding}");
    }
    // nextest refuses `--no-fail-fast` beside `--no-run`, so the gate adds it.
    assert!(!binding.contains("--no-fail-fast"), "{binding}");

    let unit = nix
        .split_once("\"test:unit\" = {")
        .expect("devenv.nix has test:unit")
        .1;
    let exec = unit
        .lines()
        .find_map(|l| l.trim().strip_prefix("exec = \""))
        .expect("test:unit has an exec");
    assert!(
        exec.starts_with("${unitBuild} --no-fail-fast;"),
        "test:unit: {exec}"
    );

    let processes = nix
        .split_once("processes = lib.mkIf (!config.devenv.isTesting) {")
        .expect("the processes are left out of devenv test, which starts them")
        .1;
    let warm = processes
        .lines()
        .find_map(|l| l.trim().strip_prefix("warm.exec = "))
        .expect("processes has warm");
    assert_eq!(
        warm,
        "\"exec bash \\\"$DEVENV_ROOT/scripts/warm.sh\\\" watch ${lib.escapeShellArg warmBuild}\";"
    );
    // The watcher builds what `test:unit` runs: the suite, and on macOS the
    // event rig, each with the gate's own string.
    let bound = |name: &str| {
        nix.lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .and_then(|l| l.strip_suffix(';'))
            .unwrap_or_else(|| panic!("devenv.nix binds {name}"))
            .to_owned()
    };
    assert_eq!(
        bound("warmBuild = "),
        "if isDarwin then \"${unitBuild} --no-run && ${rigBuild} --no-run\" else \"${unitBuild} --no-run\""
    );
    assert_eq!(
        bound("rigRun = "),
        "if isDarwin then \" ${rigBuild}; c=$?;\" else \" c=0;\""
    );
    let rig = bound("rigBuild = ");
    for word in [
        "RUSTFLAGS='-D warnings'",
        "cargo nextest run",
        "--cargo-profile release-probe",
        "--features effect-probe",
        "--test event_rig",
    ] {
        assert!(rig.contains(word), "rigBuild lacks {word}: {rig}");
    }
    assert!(
        exec.contains(
            "${rigRun} [ \\\"$a\\\" -eq 0 ] && [ \\\"$b\\\" -eq 0 ] && [ \\\"$c\\\" -eq 0 ]"
        ),
        "test:unit runs the rig and fails on it: {exec}"
    );

    assert!(nix.contains("CARGO_TARGET_DIR = \"./target\";"));
    let build = sh.split_once("\nbuild)\n").expect("warm.sh has build").1;
    assert!(
        build.contains("cd \"$root\" || exit 1"),
        "the build runs from the root"
    );
    assert!(
        build.contains("-- bash -c \"$2\" &"),
        "the build is BUILD itself"
    );
    for word in ["RUSTFLAGS", "CARGO_", "--release", "--profile"] {
        assert!(
            !sh.contains(word),
            "warm.sh names {word}, which only unitBuild may"
        );
    }

    // The time bound is nextest's, the gate's only one.
    let nextest = read(&repo.join(".config/nextest.toml"));
    assert!(nextest.contains("slow-timeout = { period = \"60s\", terminate-after = 20 }"));
    assert!(sh.contains("\ntimeout=1200\n"));
}
