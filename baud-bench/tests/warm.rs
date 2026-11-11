//! The warm build (`scripts/warm.sh`) and the guard every measurement runs
//! (`baud_bench::warm`): a measurement refuses beside an unpaused watcher or
//! build and runs once it is paused, a pause stops the whole build or says it
//! did not, every measuring `main` asks, and the watcher builds exactly what
//! `devenv test` builds.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use baud_bench::warm::{self, PAUSE, Pid, Warm};

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
        pid: Pid(pid),
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
    fakes: Vec<Child>,
}

impl Drop for Scene {
    fn drop(&mut self) {
        for file in &self.pids {
            if let Ok(pid) = std::fs::read_to_string(file) {
                let _ = Command::new("kill").args(["-9", pid.trim()]).status();
            }
        }
        for child in self.fakes.iter_mut().chain(self.child.as_mut()) {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Scene {
    fn new(name: &str) -> Scene {
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
        Scene {
            dir,
            child: None,
            pids: Vec::new(),
            fakes: Vec::new(),
        }
    }

    /// `new`, `spawn` and `wait_for`.
    fn start(name: &str, build: &str, pids: &[&str]) -> Scene {
        let mut scene = Scene::new(name);
        scene.spawn(build);
        scene.wait_for(pids);
        scene
    }

    /// `warm.sh build` with `{dir}` in `build` replaced by the folder, its
    /// output in `build.out` there.
    fn spawn(&mut self, build: &str) {
        let build = build.replace("{dir}", &self.dir.display().to_string());
        let out = std::fs::File::create(self.dir.join("build.out")).unwrap();
        self.child = Some(
            Command::new("bash")
                .arg(self.script())
                .args(["build", &build])
                .stdout(out)
                .spawn()
                .unwrap(),
        );
    }

    /// Wait for each of `pids`, files under the folder the build writes, to
    /// be written. bounded compiles its watcher on a first run.
    fn wait_for(&mut self, pids: &[&str]) {
        let until = Instant::now() + Duration::from_secs(120);
        for file in pids {
            let file = self.dir.join(file);
            self.pids.push(file.clone());
            while std::fs::read_to_string(&file)
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                let ended = self.child.as_mut().unwrap().try_wait().unwrap();
                assert!(ended.is_none(), "warm.sh build ended first: {ended:?}");
                assert!(Instant::now() < until, "the fake build never started");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    /// A process that reads as a live `cargo test` started in `cwd`: a shell
    /// script named `cargo`, given `test`, that sleeps until killed.
    fn fake_test_run(&mut self, cwd: &Path) {
        let bin = self.dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let cargo = bin.join("cargo");
        std::fs::write(&cargo, "while :; do sleep 1; done\n").unwrap();
        std::fs::create_dir_all(cwd).unwrap();
        self.fakes.push(
            Command::new("/bin/sh")
                .arg(&cargo)
                .arg("test")
                .current_dir(cwd)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
    }

    fn script(&self) -> PathBuf {
        self.dir.join("scripts/warm.sh")
    }

    fn pid(&self, file: &str) -> Pid {
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

fn alive(pid: Pid) -> bool {
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
    let build = Pid(scene.child.as_ref().unwrap().id());

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

/// A build waits for a test run started in its own checkout, whose binaries
/// it would relink, and for no other: not one in another folder, and not one
/// in a worktree under `.claude/`, which builds in its own target directory.
#[test]
fn a_build_waits_for_a_test_run_in_its_own_checkout_alone() {
    let mut scene = Scene::new("waits");
    let outside = scene.dir.with_extension("elsewhere");
    scene.fake_test_run(&outside);
    scene.fake_test_run(&scene.dir.join(".claude/worktrees/lane"));
    let build = "echo $$ >'{dir}/sleeper.pid'; exec sleep 600";
    scene.spawn(build);
    scene.wait_for(&["sleeper.pid"]);
    assert!(scene.run("pause").status.success());
    scene.ended();
    let _ = std::fs::remove_dir_all(&outside);

    assert!(scene.run("resume").status.success());
    std::fs::remove_file(scene.dir.join("sleeper.pid")).unwrap();
    let dir = scene.dir.clone();
    scene.fake_test_run(&dir);
    scene.spawn(build);
    let until = Instant::now() + Duration::from_secs(120);
    while !read(&scene.dir.join("build.out")).contains("warm: waiting for the test run to end") {
        assert!(Instant::now() < until, "the build did not wait");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !scene.dir.join("sleeper.pid").exists(),
        "it built beside the test run"
    );

    // The test run ends; the build starts.
    let mut here = scene.fakes.pop().unwrap();
    here.kill().unwrap();
    here.wait().unwrap();
    scene.wait_for(&["sleeper.pid"]);
    assert!(scene.run("pause").status.success());
    scene.ended();
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

    let pids: Vec<Pid> = scene.refused().iter().map(|w| w.pid).collect();
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

    // `warm-run` runs the watcher in the foreground. `devenv up` no longer
    // starts it: its processes are bacon and cargo-watch, each refusing to
    // start without `BAUD_WATCH`, the folder whose `.paused` the benches read.
    let processes = nix
        .split_once("processes = lib.mkIf (!config.devenv.isTesting) {")
        .expect("the processes are left out of devenv test, which starts them")
        .1
        .split_once("\n  };")
        .expect("the processes block ends")
        .0;
    assert!(!processes.contains("warm.exec"), "{processes}");
    for job in [
        "bacon.exec = noJob \"bacon\"",
        "cargo-watch.exec = noJob \"cargo-watch\"",
    ] {
        assert!(
            processes.contains(job),
            "processes lacks {job}: {processes}"
        );
    }
    let no_job = nix
        .split_once("noJob = name: ''")
        .expect("devenv.nix binds noJob")
        .1
        .split_once("'';")
        .expect("noJob ends")
        .0;
    for var in ["BAUD_WATCH_JOB", "BAUD_WATCH:-"] {
        assert!(
            no_job.contains(var),
            "noJob does not refuse without {var}: {no_job}"
        );
    }
    // cargo-watch watches the whole repository, and its walk never enters the
    // agents' sessions, devenv's state, git or a build folder: a `target/`
    // in the tree held its first walk past four minutes.
    assert!(
        processes.contains("cargo watch --watch-when-idle -d 5 ${watchIgnores} -s"),
        "{processes}"
    );
    assert!(
        !nix.contains("watchPaths"),
        "cargo-watch names no paths: it watches the repository"
    );
    assert!(!processes.contains(" -w "), "{processes}");
    let ignores = nix
        .split_once(
            "watchIgnores = lib.concatMapStringsSep \" \" (p: \"-i ${lib.escapeShellArg p}\") [",
        )
        .expect("devenv.nix binds watchIgnores")
        .1
        .split_once("];")
        .expect("watchIgnores ends")
        .0;
    let ignores: Vec<&str> = ignores
        .lines()
        .map(|l| l.trim().trim_matches('"'))
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(ignores, [".claude/", ".devenv*", ".git/", "target*"]);
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
        "\"exec bash \\\"$DEVENV_ROOT/scripts/warm.sh\\\" watch ${lib.escapeShellArg warmBuild}\""
    );
    // The watcher builds what `test:unit` runs: the suite, and on macOS the
    // event rig, each with the gate's own string.
    assert_eq!(
        bound("warmBuild = "),
        "if isDarwin then \"${unitBuild} --no-run && ${rigBuild} --no-run\" else \"${unitBuild} --no-run\""
    );
    // ... and runs every row of it: with `BAUD_RIG_ROWS` set, which would
    // run only the rows it names, the step refuses.
    assert_eq!(
        bound("rigRun = "),
        r#"if isDarwin then " if [ -n \"\${BAUD_RIG_ROWS+set}\" ]; then echo 'test:unit: BAUD_RIG_ROWS is set, and the rig would run only the rows it names: the gate runs every row' >&2; c=1; else ${rigBuild}; c=$?; fi;" else " c=0;""#
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
            "warm.sh names {word}, which only devenv.nix's strings may"
        );
    }

    // The time bound is nextest's, the gate's only one.
    let nextest = read(&repo.join(".config/nextest.toml"));
    assert!(nextest.contains("slow-timeout = { period = \"60s\", terminate-after = 20 }"));
    assert!(sh.contains("\ntimeout=1200\n"));
}

/// A fresh, empty folder for one test, under the system's temporary folder.
fn watch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("baud-watch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    dir
}

/// The watchers (bacon, cargo-watch) are not paused unless their folder holds
/// `.paused`. An idle watcher counts, with no job pid in the folder at all:
/// the next save starts a run halfway through the measurement, so a check of
/// a live pid would pass exactly the case that must refuse.
#[test]
fn an_unpaused_watch_folder_refuses_even_with_no_job_running() {
    let dir = watch_dir("unpaused");
    assert!(!dir.join("clippy.pid").exists() && !dir.join("test.pid").exists());
    let why = warm::watch_refusal(Some(&dir)).expect("an unpaused watcher refuses");
    assert!(why.contains("watch-pause.sh on"), "{why}");
    assert!(why.contains(&dir.display().to_string()), "{why}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_paused_watch_folder_lets_a_measurement_run() {
    let dir = watch_dir("paused");
    std::fs::write(dir.join(".paused"), "").expect("write .paused");
    assert_eq!(warm::watch_refusal(Some(&dir)), None);
    std::fs::remove_dir_all(&dir).ok();
}

/// With no `BAUD_WATCH` (another machine, CI) there is nothing to read.
#[test]
fn no_watch_folder_named_is_no_refusal() {
    assert_eq!(warm::watch_refusal(None), None);
}
