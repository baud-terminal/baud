//! The warm build (`scripts/warm.sh`) and the guard every measurement runs
//! (`baud_bench::warm`): a measurement refuses beside an unpaused watcher or
//! build and runs once it is paused, a pause stops the whole build or says it
//! did not, every measuring `main` asks, and the watcher builds exactly what
//! `devenv test` builds.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use baud_bench::warm::{self, PAUSE, Warm};

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
  x07 bash /r/scripts/warm.sh build cargo
";
    let r = Some(PathBuf::from("/r"));
    let expect = |pid: u32, root: &Option<PathBuf>| Warm {
        pid,
        root: root.clone(),
    };

    // Nothing paused: the watcher and its build, and the relative spelling,
    // whose checkout cannot be read. Not the pause, a look-alike script, an
    // editor, or a line whose pid is not a number.
    assert_eq!(
        warm::unpaused(listing, |_| false),
        [expect(101, &r), expect(102, &r), expect(106, &None)]
    );
    // /r paused: only the one no pause can be read for is left.
    assert_eq!(
        warm::unpaused(listing, |root| root == Path::new("/r")),
        [expect(106, &None)]
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

/// A build's processes are found by the variable in their environment, as
/// `/proc/<pid>/environ` spells it, whatever their command lines say.
#[test]
fn a_build_is_named_by_its_environment() {
    assert_eq!(
        warm::build_root(b"PATH=/bin\0BAUD_WARM_BUILD=/r\0HOME=/h\0"),
        Some(PathBuf::from("/r"))
    );
    assert_eq!(warm::build_root(b"PATH=/bin\0HOME=/h\0"), None);
    assert_eq!(warm::build_root(b"BAUD_WARM_BUILDX=/r\0"), None);
    assert_eq!(warm::build_root(b"X=BAUD_WARM_BUILD=/r\0"), None);
    assert_eq!(warm::build_root(b""), None);
}

/// A copy of the scripts in a folder of its own, `warm.sh build` running a
/// fake build in it under the real `scripts/bounded`, and everything it
/// started killed by pid at the end, whatever the test found.
struct Scene {
    dir: PathBuf,
    child: Option<Child>,
    pids: Vec<PathBuf>,
}

impl Drop for Scene {
    fn drop(&mut self) {
        for file in &self.pids {
            if let Ok(pid) = std::fs::read_to_string(file) {
                let _ = Command::new("kill").args(["-9", pid.trim()]).status();
            }
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Scene {
    /// `build` runs with `{dir}` replaced by the folder, and the scene waits
    /// for each of `pids`, files under it the build writes, to be written.
    fn start(name: &str, build: &str, pids: &[&str]) -> Scene {
        let repo = repo();
        let made = std::env::temp_dir().join(format!("baud-warm-{name}-{}", std::process::id()));
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
        let mut scene = Scene {
            pids: pids.iter().map(|p| dir.join(p)).collect(),
            dir,
            child: None,
        };
        let build = build.replace("{dir}", &scene.dir.display().to_string());
        scene.child = Some(
            Command::new("bash")
                .arg(scene.script())
                .args(["build", &build])
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        // bounded compiles its watcher on a first run; then the build starts.
        let until = Instant::now() + Duration::from_secs(120);
        for file in scene.pids.clone() {
            while std::fs::read_to_string(&file)
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                let ended = scene.child.as_mut().unwrap().try_wait().unwrap();
                assert!(ended.is_none(), "warm.sh build ended first: {ended:?}");
                assert!(Instant::now() < until, "the fake build never started");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        scene
    }

    fn script(&self) -> PathBuf {
        self.dir.join("scripts/warm.sh")
    }

    fn pid(&self, file: &str) -> u32 {
        read(&self.dir.join(file)).trim().parse().unwrap()
    }

    fn run(&self, verb: &str) -> std::process::Output {
        Command::new("bash")
            .arg(self.script())
            .arg(verb)
            .output()
            .unwrap()
    }

    /// The guard over this machine's real process list, narrowed to this
    /// folder, so a watcher elsewhere on the machine changes nothing here.
    fn refused(&self) -> Vec<Warm> {
        let here = format!("{}/scripts/warm.sh", self.dir.display());
        let listing: String = warm::listing()
            .unwrap()
            .lines()
            .filter(|l| l.contains(&here))
            .map(|l| format!("{l}\n"))
            .collect();
        let mut refused = warm::unpaused(&listing, |root| root.join(PAUSE).exists());
        refused.extend(
            warm::building()
                .into_iter()
                .filter(|w| w.root.as_deref() == Some(&self.dir)),
        );
        refused
    }

    /// Wait for `warm.sh build` to end, as it does once its tree has.
    fn ended(&mut self) -> std::process::ExitStatus {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < until,
                "warm.sh build did not end after the pause"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The live half: a build in flight refuses a measurement, `warm.sh pause`
/// stops it, and then a measurement may start.
#[test]
fn a_live_build_refuses_a_measurement_until_warm_pause_stops_it() {
    let mut scene = Scene::start(
        "live",
        "echo $$ >'{dir}/sleeper.pid'; exec sleep 600",
        &["sleeper.pid"],
    );
    let sleeper = scene.pid("sleeper.pid");
    let build = scene.child.as_ref().unwrap().id();

    // Red: a build in flight and no pause.
    let refused = scene.refused();
    assert!(
        refused.contains(&Warm {
            pid: build,
            root: Some(scene.dir.clone()),
        }),
        "the build in flight was not seen: {refused:?}"
    );

    let pause = scene.run("pause");
    assert!(pause.status.success(), "{pause:?}");
    assert!(scene.dir.join(PAUSE).exists());

    // The pause stopped the build: the fake is gone and so is warm.sh.
    assert!(!alive(sleeper), "the fake build survived the pause");
    scene.ended();

    // Green: paused, a measurement may start.
    assert_eq!(scene.refused(), []);

    // One line says so, and that no watcher is running here.
    let status = scene.run("status");
    assert_eq!(status.status.code(), Some(1), "{status:?}");
    assert_eq!(
        String::from_utf8_lossy(&status.stdout),
        "warm: no watcher, paused, no build running\n"
    );

    // A build started while paused starts nothing.
    let again = Command::new("bash")
        .arg(scene.script())
        .args(["build", "exit 9"])
        .output()
        .unwrap();
    assert!(again.status.success(), "{again:?}");

    let resume = scene.run("resume");
    assert!(resume.status.success(), "{resume:?}");
    assert!(!scene.dir.join(PAUSE).exists());
}

/// **Linux: a pause stops what bounded does not reach.** There bounded's
/// helper is systemd-run or `setsid timeout`, which a signal to bounded's pid
/// does not reach, and a process whose parent has exited is out of any tree.
/// The build leaves one of those behind, `orphan.pid`, beside its own
/// `sleeper.pid`. The guard sees both by their environment, the pause stops
/// both, and `warm.sh build` ends only after they have.
#[cfg(target_os = "linux")]
#[test]
fn on_linux_a_pause_stops_the_whole_build() {
    let mut scene = Scene::start(
        "orphan",
        "( sleep 600 & echo $! >'{dir}/orphan.pid' ); echo $$ >'{dir}/sleeper.pid'; exec sleep 600",
        &["orphan.pid", "sleeper.pid"],
    );
    let orphan = scene.pid("orphan.pid");
    let sleeper = scene.pid("sleeper.pid");

    let pids: Vec<u32> = scene.refused().iter().map(|w| w.pid).collect();
    assert!(
        pids.contains(&orphan) && pids.contains(&sleeper),
        "{pids:?}"
    );

    let pause = scene.run("pause");
    assert!(pause.status.success(), "{pause:?}");
    assert!(!alive(sleeper), "the build survived the pause");
    assert!(!alive(orphan), "the orphan survived the pause");
    scene.ended();
    assert_eq!(scene.refused(), []);
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
/// gate's. Then the no-LTO arm, in a directory of its own.
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

    // `devenv up`'s process and `warm-run` are one watcher.
    let processes = nix
        .split_once("processes = lib.mkIf (!config.devenv.isTesting) {")
        .expect("the processes are left out of devenv test, which starts them")
        .1;
    let warm = processes
        .lines()
        .find_map(|l| l.trim().strip_prefix("warm.exec = "))
        .expect("processes has warm");
    assert_eq!(warm, "watch;");
    assert!(nix.contains("\n  scripts.warm-run.exec = watch;\n"));
    let bound = |name: &str| {
        nix.lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .and_then(|l| l.strip_suffix(';'))
            .unwrap_or_else(|| panic!("devenv.nix binds {name}"))
            .to_owned()
    };
    assert_eq!(
        bound("watch = "),
        "\"exec bash \\\"$DEVENV_ROOT/scripts/warm.sh\\\" watch ${lib.escapeShellArg watchBuild}\""
    );
    assert_eq!(
        bound("watchBuild = "),
        "\"${warmBuild} && ${noltoBuild} --no-run\""
    );
    // The watcher builds what `test:unit` runs: the suite, and on macOS the
    // event rig, each with the gate's own string.
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
    // The no-LTO arm: the re-runs' variables, in `target/nolto`.
    let nolto = bound("noltoBuild = ");
    for word in [
        "CARGO_TARGET_DIR=./target/nolto ",
        "CARGO_PROFILE_RELEASE_LTO=off ",
        "CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 ",
        "RUSTFLAGS='-D warnings' ",
        "cargo nextest run --release",
        "${scope}",
    ] {
        assert!(nolto.contains(word), "noltoBuild lacks {word}: {nolto}");
    }

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
            "warm.sh names {word}, which only devenv.nix's strings may"
        );
    }

    // The time bound is nextest's, the gate's only one.
    let nextest = read(&repo.join(".config/nextest.toml"));
    assert!(nextest.contains("slow-timeout = { period = \"60s\", terminate-after = 20 }"));
    assert!(sh.contains("\ntimeout=1200\n"));
}
