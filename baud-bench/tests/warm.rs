//! The warm build (`scripts/warm.sh`) and the guard every measurement runs
//! (`baud_bench::warm`): a measurement refuses beside an unpaused watcher or
//! build and runs once it is paused, a pause stops the whole build or says it
//! did not, every measuring `main` asks, and the watcher builds exactly what
//! `devenv test` builds. A pinned measurement is judged by the load on its
//! own cores and the GPU alone, over a machine given as text.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use baud_bench::warm::{
    self, Busy, BusyPct, CoreSet, Cpu, Machine, PAUSE, Pid, Placement, Read, Verdict, Warm,
};

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
    scene.fake_test_run(&scene.dir.join(".claude/other"));
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
    // in the tree held its first walk past four minutes. A change during a run
    // queues one run after it (`--no-restart`); `--watch-when-idle` dropped it.
    assert!(
        processes.contains("cargo watch --no-restart -d 5 ${watchIgnores} -s"),
        "{processes}"
    );
    assert!(!processes.contains("--watch-when-idle"), "{processes}");
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
    // The watcher builds what `test:unit` runs: the suite, the band reserve
    // test and on macOS the event rig, each with the gate's own string.
    assert_eq!(
        bound("warmBuild = "),
        "if isDarwin then \"${unitBuild} --no-run && ${rigBuild} --no-run && ${pinBuild} --no-run\" else \"${unitBuild} --no-run && ${pinBuild} --no-run\""
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
            "${rigRun} ${pinBuild}; d=$?; [ \\\"$a\\\" -eq 0 ] && [ \\\"$b\\\" -eq 0 ] && [ \\\"$c\\\" -eq 0 ] && [ \\\"$d\\\" -eq 0 ]"
        ),
        "test:unit runs the rig and the band reserve test, and fails on either: {exec}"
    );
    let pin = bound("pinBuild = ");
    for word in [
        "RUSTFLAGS='-D warnings'",
        "cargo nextest run",
        "--cargo-profile release-probe",
        "--features probe-band-pin,alloc",
        "--test band_reserve",
    ] {
        assert!(pin.contains(word), "pinBuild lacks {word}: {pin}");
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

/// The set a CPU list names.
fn cores(list: &str) -> CoreSet {
    warm::cpu_list(list).unwrap_or_else(|e| panic!("{list}: {e}"))
}

/// A CPU list reads as taskset and the kernel write it, and prints back the
/// same way.
#[test]
fn a_cpu_list_reads_as_the_kernel_writes_it() {
    let set = warm::cpu_list("0-3,8,10-11\n").expect("a list");
    let each: CoreSet = [0, 1, 2, 3, 8, 10, 11].map(Cpu).into_iter().collect();
    assert_eq!(set, each);
    assert_eq!(set.to_string(), "0-3,8,10-11");
    assert_eq!(
        warm::cpu_list("57").unwrap(),
        [Cpu(57)].into_iter().collect()
    );
    assert_eq!(cores("57,121").to_string(), "57,121");
    for bad in ["", "3-1", "a", "1,,2", "1-", "-4"] {
        assert!(warm::cpu_list(bad).is_err(), "{bad:?} read as a list");
    }
    // The affinity is the status file's `Cpus_allowed_list` line.
    let status =
        "Name:\tbench\nCpus_allowed:\t0300\nCpus_allowed_list:\t56-57\nMems_allowed_list:\t0\n";
    assert_eq!(warm::allowed(status).unwrap(), cores("56-57"));
    assert!(warm::allowed("Name:\tbench\n").is_err());
}

/// A measurement is pinned when its affinity leaves out an online CPU, and
/// its cores are then the online CPUs it may run on.
#[test]
fn a_measurement_is_pinned_when_its_affinity_leaves_out_an_online_cpu() {
    let online = cores("0-3");
    assert_eq!(warm::placement(&online, &online), Placement::Unpinned);
    assert_eq!(warm::placement(&cores("0-5"), &online), Placement::Unpinned);
    assert_eq!(
        warm::placement(&cores("2,5"), &online),
        Placement::Pinned(cores("2"))
    );
}

/// A core's SMT siblings share it, so they are the measurement's too; a
/// topology file that is absent or cannot be read is named, never passed.
#[test]
fn own_cores_take_their_smt_siblings() {
    let topology = |cpu: Cpu| match cpu {
        Cpu(57) => Read::Text("57,121\n".into()),
        Cpu(58) => Read::Text("58-59\n".into()),
        Cpu(60) => Read::Absent,
        _ => Read::Unreadable("Permission denied".into()),
    };
    assert_eq!(
        warm::siblings(&cores("57-58"), topology).unwrap(),
        cores("57-59,121")
    );
    let absent = warm::siblings(&cores("60"), topology).unwrap_err();
    assert!(
        absent.contains("/sys/devices/system/cpu/cpu60/topology/thread_siblings_list"),
        "{absent}"
    );
    let unreadable = warm::siblings(&cores("61"), topology).unwrap_err();
    assert!(unreadable.contains("cpu61/topology"), "{unreadable}");
    assert!(unreadable.contains("Permission denied"), "{unreadable}");
}

const STAT_BEFORE: &str = "\
cpu  1000 0 1000 8000 0 0 0 0 0 0
cpu0 100 0 100 800 0 0 0 0 0 0
cpu57 0 0 0 1000 0 0 0 0 0 0
cpu121 0 0 0 1000 0 0 0 0 0 0
cpu122 0 0 0 1000 0 0 0 0 0 0
intr 1 2 3
";

const STAT_AFTER: &str = "\
cpu  2000 0 1000 8000 0 0 0 0 0 0
cpu0 200 0 100 800 0 0 0 0 0 0
cpu57 90 0 8 1000 2 0 0 0 0 0
cpu121 4 0 0 1090 6 0 0 0 0 0
cpu122 0 0 0 1000 0 0 0 0 0 0
intr 4 5 6
";

/// Busy is the share of a core's ticks that are neither idle nor iowait
/// between two reads of /proc/stat, as the harness's quiet check counts it.
#[test]
fn busy_is_the_share_of_ticks_neither_idle_nor_iowait() {
    let max = BusyPct(5.0);
    // cpu57: 100 ticks, 2 of them iowait: 98%. cpu121: 100 ticks, 96 idle or
    // iowait: 4%, under the line. cpu122 did not tick: 0%. cpu0 is busy and
    // not the measurement's.
    assert_eq!(
        warm::busy(STAT_BEFORE, STAT_AFTER, &cores("57,121-122"), max).unwrap(),
        [Busy {
            cpu: Cpu(57),
            pct: BusyPct(98.0)
        }]
    );
    assert_eq!(
        warm::busy(STAT_BEFORE, STAT_AFTER, &cores("121"), BusyPct(4.0)).unwrap(),
        [Busy {
            cpu: Cpu(121),
            pct: BusyPct(4.0)
        }]
    );
    // A core neither read names is not known to be quiet.
    let missing = warm::busy(STAT_BEFORE, STAT_AFTER, &cores("57,64"), max).unwrap_err();
    assert!(missing.contains("cpu64"), "{missing}");
}

/// A pinned measurement that counts reads no load: beside a busy own core
/// it measures, where the same machine timed, or with no mode set, refuses.
#[test]
fn a_counting_measurement_reads_no_load_and_a_timed_one_does() {
    let busy = |mode: Option<&str>| {
        let mut fake = Fake::pinned();
        fake.file("/proc/stat", &[STAT_BEFORE, STAT_AFTER]);
        if let Some(mode) = mode {
            fake.vars.insert("BAUD_MEASURES".into(), mode.into());
        }
        fake
    };
    let fake = busy(Some("counts"));
    assert_eq!(warm::measures(&fake), Ok(warm::Measures::Counts));
    assert_eq!(fake.verdict(), Verdict::Counting);
    assert!(
        fake.slept.borrow().is_empty(),
        "a counting run samples nothing"
    );
    assert!(
        fake.read
            .borrow()
            .iter()
            .all(|p| p != Path::new("/proc/stat") && !p.starts_with("/sys/class/drm")),
        "{:?}",
        fake.read.borrow()
    );

    for mode in [Some("time"), None] {
        let fake = busy(mode);
        assert_eq!(warm::measures(&fake), Ok(warm::Measures::Time));
        let why = fake.refusal();
        assert!(why.contains("cpu57 98% busy"), "{mode:?}: {why}");
    }
}

/// A mode that is neither `time` nor `counts` refuses, naming it, pinned
/// or not.
#[test]
fn an_unknown_mode_refuses_naming_it() {
    for value in ["count", "Time", ""] {
        let mut fake = Fake::pinned();
        fake.vars.insert("BAUD_MEASURES".into(), value.into());
        let why = fake.refusal();
        assert!(why.contains(&format!("BAUD_MEASURES: `{value}`")), "{why}");
        fake.file("/proc/self/status", &["Cpus_allowed_list:\t0-127\n"]);
        let why = fake.refusal();
        assert!(
            why.contains(&format!("BAUD_MEASURES: `{value}`")),
            "unpinned: {why}"
        );
    }
}

/// A counting run gives no figure on the clock and a timed one does, and
/// the benches the counter gate runs print theirs through that choice.
#[test]
fn a_counting_run_prints_no_rate_and_a_timed_one_does() {
    let rate = || String::from("cat new: 512.0 MB/s");
    assert_eq!(warm::Measures::Counts.figure(rate), None);
    assert_eq!(warm::Measures::Time.figure(rate), Some(rate()));
    for bench in ["ingest2", "frame"] {
        let text = read(&repo().join(format!("baud-bench/benches/{bench}.rs")));
        assert!(text.contains("refuse_while_counting("), "{bench}");
        assert!(text.contains("measures.figure("), "{bench}");
    }
}

/// The GPU refuses only when both reads are at or above the line: one busy
/// read is a frame the compositor drew.
#[test]
fn the_gpu_refuses_only_when_both_reads_are_busy() {
    let max = BusyPct(10.0);
    assert!(warm::gpu_busy([BusyPct(12.0), BusyPct(41.0)], max));
    assert!(warm::gpu_busy([BusyPct(10.0), BusyPct(10.0)], max));
    assert!(!warm::gpu_busy([BusyPct(12.0), BusyPct(6.0)], max));
    assert!(!warm::gpu_busy([BusyPct(6.0), BusyPct(12.0)], max));
    assert_eq!(warm::percent("37\n"), Ok(BusyPct(37.0)));
    assert!(warm::percent("busy").is_err());
}

/// A machine given as text: each file a queue of reads, the last repeated,
/// the DRM folder's entries one a line, the environment, and a log of what was read
/// and how long it slept.
struct Fake {
    files: RefCell<HashMap<PathBuf, VecDeque<Read>>>,
    drm: Read,
    vars: HashMap<String, String>,
    read: RefCell<Vec<PathBuf>>,
    slept: RefCell<Vec<Duration>>,
}

impl Fake {
    /// CPU 57 of 0-127, its sibling 121, the stat reads above and a card1
    /// whose GPU reads 4 then 6.
    fn pinned() -> Fake {
        let mut fake = Fake {
            files: RefCell::new(HashMap::new()),
            drm: Read::Text("card1\ncard1-DP-1\nrenderD128\nversion\n".into()),
            vars: HashMap::new(),
            read: RefCell::new(Vec::new()),
            slept: RefCell::new(Vec::new()),
        };
        fake.file("/proc/self/status", &["Cpus_allowed_list:\t57\n"]);
        fake.file("/sys/devices/system/cpu/online", &["0-127\n"]);
        fake.file(
            "/sys/devices/system/cpu/cpu57/topology/thread_siblings_list",
            &["57,121\n"],
        );
        fake.file("/proc/stat", &[STAT_BEFORE, STAT_BEFORE]);
        fake.file(GPU, &["4\n", "6\n"]);
        fake
    }

    fn file(&mut self, path: &str, reads: &[&str]) {
        self.set(
            path,
            reads.iter().map(|t| Read::Text((*t).into())).collect(),
        );
    }

    fn set(&mut self, path: &str, reads: VecDeque<Read>) {
        self.files.get_mut().insert(PathBuf::from(path), reads);
    }

    fn verdict(&self) -> Verdict {
        warm::on_own_cores(self)
    }

    fn refusal(&self) -> String {
        match self.verdict() {
            Verdict::Refused(why) => why,
            other => panic!("not refused: {other:?}"),
        }
    }
}

const GPU: &str = "/sys/class/drm/card1/device/gpu_busy_percent";

impl Machine for Fake {
    fn read(&self, path: &Path) -> Read {
        self.read.borrow_mut().push(path.to_path_buf());
        let mut files = self.files.borrow_mut();
        let Some(reads) = files.get_mut(path) else {
            return Read::Absent;
        };
        if reads.len() > 1 {
            reads.pop_front().unwrap()
        } else {
            reads.front().cloned().unwrap_or(Read::Absent)
        }
    }

    fn drm(&self) -> Read {
        self.drm.clone()
    }

    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn sleep(&self, how_long: Duration) {
        self.slept.borrow_mut().push(how_long);
    }
}

/// Pinned and quiet: load on other cores, a warm build anywhere, changes
/// nothing. It samples the CPUs for 500 ms and the GPU twice 100 ms apart.
#[test]
fn a_pinned_measurement_on_quiet_cores_runs_whatever_runs_elsewhere() {
    let mut fake = Fake::pinned();
    // cpu0 went from idle to fully busy; it is not the measurement's.
    fake.file(
        "/proc/stat",
        &[
            STAT_BEFORE,
            STAT_AFTER
                .replace("cpu57 90 0 8 1000 2", "cpu57 0 0 0 1100 0")
                .replace("cpu121 4 0 0 1090 6", "cpu121 0 0 0 1100 0")
                .as_str(),
        ],
    );
    assert_eq!(fake.verdict(), Verdict::Quiet);
    assert_eq!(
        *fake.slept.borrow(),
        [Duration::from_millis(500), Duration::from_millis(100)]
    );
    // Only card1's own device is read; a connector's is not a GPU.
    let read = fake.read.borrow();
    assert_eq!(
        read.iter()
            .filter(|p| p.ends_with("gpu_busy_percent"))
            .count(),
        2
    );
    assert!(
        read.iter()
            .all(|p| !p.starts_with("/sys/class/drm/card1-DP-1"))
    );
}

/// Load on an own core, or on its SMT sibling, refuses and names the core
/// and its busy percent.
#[test]
fn load_on_an_own_core_or_its_sibling_refuses_naming_it() {
    let mut fake = Fake::pinned();
    fake.file("/proc/stat", &[STAT_BEFORE, STAT_AFTER]);
    let why = fake.refusal();
    assert!(why.contains("cpu57 98% busy"), "{why}");
    assert!(!why.contains("cpu121"), "4% is under the line: {why}");
    assert!(!why.contains("cpu0 "), "{why}");

    let mut fake = Fake::pinned();
    fake.vars.insert("BAUD_QUIET_BUSY".into(), "3".into());
    fake.file(
        "/proc/stat",
        &[
            STAT_BEFORE,
            &STAT_AFTER.replace("cpu57 90 0 8 1000 2", "cpu57 0 0 0 1100 0"),
        ],
    );
    let why = fake.refusal();
    assert!(why.contains("cpu121 4% busy"), "the sibling: {why}");
    assert!(!why.contains("cpu57 "), "{why}");
}

/// A GPU busy on both reads refuses and names the card and its readings;
/// busy on one read does not.
#[test]
fn a_busy_gpu_refuses_naming_the_card() {
    let mut fake = Fake::pinned();
    fake.file(GPU, &["12\n", "41\n"]);
    let why = fake.refusal();
    assert!(why.contains("card1 12% then 41% busy"), "{why}");

    let mut fake = Fake::pinned();
    fake.file(GPU, &["41\n", "6\n"]);
    assert_eq!(fake.verdict(), Verdict::Quiet);

    let mut fake = Fake::pinned();
    fake.vars.insert("BAUD_QUIET_GPU_BUSY".into(), "50".into());
    fake.file(GPU, &["41\n", "45\n"]);
    assert_eq!(fake.verdict(), Verdict::Quiet);
}

/// Absent and unreadable differ. No GPU file, or no DRM folder, is no GPU
/// check; a GPU file that exists and cannot be read refuses, and so does
/// any of the CPU files that cannot be read, each named.
#[test]
fn an_absent_gpu_is_no_check_and_an_unreadable_file_refuses() {
    let mut fake = Fake::pinned();
    fake.set(GPU, VecDeque::new());
    assert_eq!(fake.verdict(), Verdict::Quiet);
    assert_eq!(*fake.slept.borrow(), [Duration::from_millis(500)]);

    let mut fake = Fake::pinned();
    fake.drm = Read::Absent;
    assert_eq!(fake.verdict(), Verdict::Quiet);

    let mut fake = Fake::pinned();
    fake.drm = Read::Unreadable("Permission denied".into());
    let why = fake.refusal();
    assert!(why.contains("/sys/class/drm: Permission denied"), "{why}");

    let mut fake = Fake::pinned();
    fake.set(
        GPU,
        [Read::Unreadable("Device or resource busy".into())].into(),
    );
    let why = fake.refusal();
    assert!(
        why.contains(&format!("{GPU}: Device or resource busy")),
        "{why}"
    );

    let mut fake = Fake::pinned();
    fake.set(GPU, [Read::Text("12\n".into()), Read::Absent].into());
    let why = fake.refusal();
    assert!(why.contains(GPU), "a file gone by the second read: {why}");

    let mut fake = Fake::pinned();
    fake.file(GPU, &["12\n", "busy\n"]);
    let why = fake.refusal();
    assert!(why.contains(GPU), "{why}");

    for (path, gone) in [
        ("/proc/stat", Read::Unreadable("Permission denied".into())),
        ("/proc/stat", Read::Absent),
        (
            "/proc/self/status",
            Read::Unreadable("Permission denied".into()),
        ),
        ("/sys/devices/system/cpu/online", Read::Absent),
        (
            "/sys/devices/system/cpu/cpu57/topology/thread_siblings_list",
            Read::Unreadable("Permission denied".into()),
        ),
        (
            "/sys/devices/system/cpu/cpu57/topology/thread_siblings_list",
            Read::Absent,
        ),
    ] {
        let mut fake = Fake::pinned();
        fake.set(path, [gone.clone()].into());
        let why = fake.refusal();
        assert!(why.contains(path), "{path} {gone:?}: {why}");
    }
}

/// An unpinned measurement is not judged by its cores: it reads its
/// affinity and nothing else, and the warm-build tests decide.
#[test]
fn an_unpinned_measurement_reads_no_load() {
    let mut fake = Fake::pinned();
    fake.file("/proc/self/status", &["Cpus_allowed_list:\t0-127\n"]);
    fake.file("/proc/stat", &[STAT_BEFORE, STAT_AFTER]);
    fake.file(GPU, &["90\n"]);
    assert_eq!(fake.verdict(), Verdict::Unpinned);
    assert_eq!(
        *fake.read.borrow(),
        [
            PathBuf::from("/proc/self/status"),
            PathBuf::from("/sys/devices/system/cpu/online")
        ]
    );
    assert!(fake.slept.borrow().is_empty());
}

/// The lines and the sample come from the environment; a value that is not
/// a number refuses, naming the variable.
#[test]
fn the_lines_and_the_sample_come_from_the_environment() {
    let mut fake = Fake::pinned();
    fake.vars.insert("BAUD_QUIET_SAMPLE".into(), "200".into());
    fake.vars.insert("BAUD_QUIET_BUSY".into(), "99".into());
    fake.file("/proc/stat", &[STAT_BEFORE, STAT_AFTER]);
    assert_eq!(fake.verdict(), Verdict::Quiet);
    assert_eq!(fake.slept.borrow()[0], Duration::from_millis(200));

    for name in [
        "BAUD_QUIET_BUSY",
        "BAUD_QUIET_SAMPLE",
        "BAUD_QUIET_GPU_BUSY",
    ] {
        let mut fake = Fake::pinned();
        fake.vars.insert(name.into(), "five".into());
        let why = fake.refusal();
        assert!(why.contains(name) && why.contains("five"), "{why}");
    }
}
