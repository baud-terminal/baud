//! The programs that measure the app from outside, over stand-ins.
//!
//! Each row of `host/cases.tsv` is a command line. `host/golden/` holds what
//! the Python each program was ported from printed to stdout on that row, and
//! the row its exit code, under the same clock: `host/app.sh` is the app, and
//! moves the clock a report's wall is read from. So each table test here is
//! that side-by-side with the Python gone, and each named test below is one
//! perturbation of one parser or statistic, asserting the check fires.
//!
//! Nothing here starts the app, opens a window, or touches a real cache: the
//! cache directory comes through [`Host::cache_root`].

use baud_bench::host::{
    self, Host, Live, dock_check, idle_cpu, py, shader_cache, startup, vt_reply,
};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

mod exec;

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/host");

/// The `PATH` a stand-in runs under: the system's four directories, then
/// this process's own `PATH`, so the stand-in's `cat`, `grep` and `awk` are
/// the system's where it has them and are found on a system that keeps them
/// elsewhere (NixOS keeps none in `/usr/bin` or `/bin`).
fn stand_in_path() -> OsString {
    let mut path = OsString::from("/usr/bin:/bin:/usr/sbin:/sbin");
    if let Some(own) = std::env::var_os("PATH").filter(|p| !p.is_empty()) {
        path.push(":");
        path.push(own);
    }
    path
}

/// A row of `cases.tsv`.
struct Case {
    tool: String,
    name: String,
    code: i32,
    env: Vec<(String, String)>,
    args: Vec<String>,
}

fn cases(tool: &str) -> Vec<Case> {
    let text = fs::read_to_string(format!("{DATA}/cases.tsv")).expect("cases.tsv");
    let found: Vec<Case> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut f = l.split('\t');
            let tool = f.next().expect("a tool").to_string();
            let name = f.next().expect("a name").to_string();
            let code = f.next().expect("a code").parse().expect("a number");
            let env = f.next().expect("an environment");
            let env = if env == "-" {
                Vec::new()
            } else {
                env.split(';')
                    .map(|kv| kv.split_once('=').expect("KEY=VALUE"))
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            };
            Case {
                tool,
                name,
                code,
                env,
                args: f.map(str::to_string).collect(),
            }
        })
        .filter(|c| c.tool == tool)
        .collect();
    assert!(found.len() >= 12, "{tool}: only {} rows", found.len());
    found
}

fn case(tool: &str, name: &str) -> Case {
    cases(tool)
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no case {tool}-{name}"))
}

/// A case's directory: stand-ins, a clock and two caches.
struct Scene {
    dir: PathBuf,
}

impl Scene {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "baud-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("root/com.apple.metal")).expect("a cache");
        fs::create_dir_all(dir.join("root/com.apple.metalfe")).expect("a cache");
        fs::write(dir.join("root/com.apple.metal/marker"), "metal").expect("a marker");
        fs::write(dir.join("root/com.apple.metalfe/marker"), "metalfe").expect("a marker");
        fs::write(dir.join("plain"), "not a program").expect("a file");
        Scene { dir }
    }

    /// The stand-in app, launching what `launches/<name>.txt` says.
    fn app(&self, name: &str) -> String {
        let text = fs::read_to_string(format!("{DATA}/launches/{name}.txt"))
            .unwrap_or_else(|_| panic!("launches/{name}.txt"));
        self.app_with(name, &text)
    }

    /// The stand-in app, launching `text`.
    fn app_with(&self, name: &str, text: &str) -> String {
        let path = self.dir.join(name);
        exec::copy(Path::new(&format!("{DATA}/app.sh")), &path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("mode");
        fs::write(self.dir.join(format!("{name}.txt")), text).expect("its launches");
        path.display().to_string()
    }

    fn args(&self, c: &Case) -> Vec<String> {
        c.args
            .iter()
            .map(|a| {
                let mut a = a.clone();
                while let Some(i) = a.find("{app:") {
                    let j = i + a[i..].find('}').expect("a closing brace");
                    let path = self.app(&a[i + 5..j]);
                    a.replace_range(i..=j, &path);
                }
                let d = self.dir.display().to_string();
                a.replace("{plain}", &format!("{d}/plain"))
                    .replace("{d}", &d)
            })
            .collect()
    }

    fn env(&self, c: &Case) -> Vec<(OsString, OsString)> {
        let mut env: Vec<(OsString, OsString)> = vec![
            ("PATH".into(), stand_in_path()),
            ("HOME".into(), self.dir.clone().into()),
            ("TMPDIR".into(), self.dir.clone().into()),
        ];
        for (k, v) in &c.env {
            if !matches!(k.as_str(), "SETUP" | "STDIN" | "FAKE_RATES") {
                env.push((k.into(), v.into()));
            }
        }
        env
    }

    fn host(&self, c: &Case) -> Stand {
        let rates = c
            .env
            .iter()
            .find(|(k, _)| k == "FAKE_RATES")
            .map_or("0", |(_, v)| v.as_str());
        Stand {
            dir: self.dir.clone(),
            rates: rates.split(',').map(str::to_string).collect(),
            cpu_calls: Calls::default(),
            wall_calls: Calls::default(),
            interrupt_after: None,
            fired: Fired::No,
            interrupt_in_pause: None,
            read_pid: None,
        }
    }

    fn cache(&self, name: &str) -> PathBuf {
        self.dir.join("root").join(name)
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// How many times something was asked.
#[derive(Default)]
struct Calls(usize);

impl Calls {
    /// This call's number, from zero.
    fn next(&mut self) -> usize {
        self.0 += 1;
        self.0 - 1
    }
}

/// Whether an interrupt was reported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fired {
    No,
    Yes,
}

/// The host of `tests/host/`: its clock is the file the stand-in moves, its
/// idle rates are the row's, and nothing sleeps.
struct Stand {
    dir: PathBuf,
    rates: Vec<String>,
    cpu_calls: Calls,
    wall_calls: Calls,
    interrupt_after: Option<usize>,
    /// Whether an interrupt has been reported.
    fired: Fired,
    /// The sleep an interrupt arrives during, counting from one.
    interrupt_in_pause: Option<usize>,
    /// The process whose CPU was last asked for.
    read_pid: Option<u32>,
}

impl Host for Stand {
    fn now(&mut self) -> f64 {
        fs::read_to_string(self.dir.join("clock"))
            .ok()
            .and_then(|t| py::float(&t))
            .unwrap_or(0.0)
    }

    fn wall(&mut self) -> f64 {
        let k = self.wall_calls.next();
        100.0 + if k % 2 == 1 { 20.0 } else { 0.0 }
    }

    fn pause(&mut self, _: f64) {
        match &mut self.interrupt_in_pause {
            Some(1) => self.interrupt_after = Some(0),
            Some(n) => *n -= 1,
            None => {}
        }
    }

    fn cpu_seconds(&mut self, pid: u32) -> Option<f64> {
        self.read_pid = Some(pid);
        let k = self.cpu_calls.next();
        let r = &self.rates[(k / 2) % self.rates.len()];
        let second = k % 2 == 1;
        match r.as_str() {
            "gone0" if !second => None,
            "gone1" if second => None,
            "gone0" | "gone1" => Some(1.0),
            r => Some(
                1.0 + if second {
                    py::float(r).expect("a rate") * 20.0
                } else {
                    0.0
                },
            ),
        }
    }

    fn cache_root(&mut self) -> Result<String, String> {
        Ok(self.dir.join("root").display().to_string())
    }

    fn interrupted(&mut self) -> bool {
        match &mut self.interrupt_after {
            Some(0) => {
                self.fired = Fired::Yes;
                true
            }
            Some(n) => {
                *n -= 1;
                false
            }
            None => false,
        }
    }
}

/// `value` as `printf %b` spells it, which is how `cases.tsv` spells a byte.
fn printf_b(value: &str) -> Vec<u8> {
    Command::new("printf")
        .args(["%b", value])
        .output()
        .expect("printf")
        .stdout
}

/// A pipe already holding `bytes`, its writing end closed.
fn pipe_holding(bytes: &[u8]) -> i32 {
    let mut ends = [0; 2];
    // SAFETY: `ends` is two live descriptors' worth of storage, and `bytes`
    // a live buffer of the length passed, shorter than a pipe holds.
    unsafe {
        assert_eq!(libc::pipe(ends.as_mut_ptr()), 0);
        let wrote = libc::write(ends[1], bytes.as_ptr().cast(), bytes.len());
        assert_eq!(usize::try_from(wrote).ok(), Some(bytes.len()));
        libc::close(ends[1]);
    }
    ends[0]
}

/// One row, run: its stdout and exit code.
fn run(scene: &Scene, c: &Case) -> (String, i32) {
    let args = scene.args(c);
    let env = scene.env(c);
    let mut out = Vec::new();
    let setup = c
        .env
        .iter()
        .find(|(k, _)| k == "SETUP")
        .map(|(_, v)| v.as_str());
    match setup {
        Some("leftover") => {
            let aside = scene
                .dir
                .join("root/com.apple.metal.shader-cache-bench-aside");
            fs::create_dir_all(&aside).expect("a leftover");
            fs::write(aside.join("marker"), "older").expect("a marker");
        }
        Some("nocaches") => fs::remove_dir_all(scene.dir.join("root")).expect("no caches"),
        _ => {}
    }
    let code = match c.tool.as_str() {
        "startup" => startup::run(&args, &env, &mut scene.host(c), &mut out),
        "idle" => idle_cpu::run(&args, &env, &mut scene.host(c), &mut out),
        "shader" => shader_cache::run(&args, &env, &mut scene.host(c), &mut out),
        "dock" => dock_check::run(&args, &env, &mut out),
        "vt" => {
            let stdin = c
                .env
                .iter()
                .find(|(k, _)| k == "STDIN")
                .map_or("", |(_, v)| v.as_str());
            let fd = pipe_holding(&printf_b(stdin));
            let code = vt_reply::run(&args, fd, &mut Live::new(), &mut out);
            // SAFETY: `fd` is this function's, and nothing else closes it.
            unsafe { libc::close(fd) };
            code
        }
        other => panic!("no tool {other}"),
    };
    (String::from_utf8(out).expect("UTF-8"), code)
}

fn golden(c: &Case) -> String {
    fs::read_to_string(format!("{DATA}/golden/{}-{}.txt", c.tool, c.name))
        .unwrap_or_else(|_| panic!("no golden for {}-{}", c.tool, c.name))
}

/// Every row of `tool` prints its golden and exits with its code.
fn table(tool: &str) {
    let mut wrong = Vec::new();
    let mut rows = 0;
    for c in cases(tool) {
        let scene = Scene::new();
        let (got, code) = run(&scene, &c);
        rows += 1;
        if got != golden(&c) || code != c.code {
            wrong.push(format!(
                "{}: exit {code} against {}\n--- got\n{got}--- want\n{}",
                c.name,
                c.code,
                golden(&c)
            ));
        }
    }
    assert!(rows >= 12, "{tool}: only {rows} rows");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn startup_bench_prints_what_the_python_printed() {
    table("startup");
}

#[test]
fn idle_cpu_prints_what_the_python_printed() {
    table("idle");
}

#[test]
fn shader_cache_bench_prints_what_the_python_printed() {
    table("shader");
}

/// macOS only, because the check counts through `/usr/bin/wc -m` under a
/// `LANG` the system must carry.
#[cfg(target_os = "macos")]
#[test]
fn dock_launch_check_prints_what_the_python_printed() {
    table("dock");
}

#[test]
fn vt_reply_prints_what_the_python_printed() {
    table("vt");
}

/// A row's stdout and exit code, for a test that looks inside.
fn outcome(tool: &str, name: &str) -> (String, i32) {
    let c = case(tool, name);
    let scene = Scene::new();
    run(&scene, &c)
}

// startup-bench: the parsers.

/// **A mark is `startup: <stage> <digits and dots> ms`, and the stage is at
/// least one character before white space.**
#[test]
fn a_mark_takes_the_digits_the_line_ends_on() {
    let m = startup::mark;
    assert_eq!(m("startup: gpu 1.5 ms"), Some(("gpu", "1.5")));
    assert_eq!(
        m("startup: first present   40.0 ms"),
        Some(("first present", "40.0"))
    );
    assert_eq!(m("startup: x 1.2 3.4 ms"), Some(("x 1.2", "3.4")));
    assert_eq!(
        m("startup: walk: 0 misses 0.0 ms"),
        Some(("walk: 0 misses", "0.0"))
    );
    assert_eq!(m("startup: 1.0 ms"), None, "no stage");
    assert_eq!(m("startup: gpu  ms"), None, "no value");
    assert_eq!(
        m("startup: gpu \u{661} ms"),
        None,
        "ASCII digits: the app prints no others"
    );
    assert_eq!(
        m("startup:  2.0 ms"),
        None,
        "one space is the divider, none left"
    );
    assert_eq!(
        m("startup:   3.0 ms"),
        Some((" ", "3.0")),
        "two: the stage is a space"
    );
    assert_eq!(
        m("startup: a b1.0 ms"),
        None,
        "no white space before the digits"
    );
    assert_eq!(m("startup: gpu 1.5 ms and more"), None, "it ends on `ms`");
    assert_eq!(m("startup: gpu 1.5ms"), None);
    assert_eq!(m("startup:gpu 1.5 ms"), None);
    assert_eq!(m("startup: nbsp\u{a0}7.5 ms"), Some(("nbsp", "7.5")));
    assert_eq!(
        m("startup: fs\u{1c}8.5 ms"),
        Some(("fs", "8.5")),
        "Python's \\s takes it"
    );
}

/// The stage the wall is taken to is followed by white space, not by more
/// of a word, and a report's line has lost its `\n` by then.
#[test]
fn the_stage_ends_in_white_space() {
    let r = |l| startup::reaches(l, &["first completion", "first present"]);
    assert!(r("startup: first present 4.0 ms"));
    assert!(r("startup: first completion\t1"));
    assert!(!r("startup: first presented 4.0 ms"));
    assert!(!r("startup: first present"));
    assert!(!r("startup: second present 4.0 ms"));
    assert!(!r("first present 4.0 ms"));
}

/// A note carries a colon: it is printed with the launch's lines and is not
/// a stage.
#[test]
fn a_note_is_printed_and_is_not_a_stage() {
    let (out, code) = outcome("startup", "single-odd");
    assert_eq!(code, 0);
    assert!(
        out.contains("startup: walk: 2 map misses, 3 hits 0.0 ms\n"),
        "{out}"
    );
    assert!(!out.contains("  walk"), "{out}");
}

/// The first value of a stage printed twice is the one kept.
#[test]
fn a_stage_printed_twice_keeps_its_first_value() {
    let (out, _) = outcome("startup", "odd-lines");
    assert!(
        out.contains("  dup                  median      1.0 ms"),
        "{out}"
    );
}

/// A launch whose stage has no number is an error with what came before it
/// kept, and a note's is not read at all.
#[test]
fn a_value_that_is_no_number_stops_a_stage_but_not_a_note() {
    let (out, code) = outcome("startup", "bad-number");
    assert_eq!((out.as_str(), code), ("", 1));
    let (out, code) = outcome("startup", "bad-number-in-a-note");
    assert_eq!(code, 1, "the second line is a stage named dot: {out}");
    let scene = Scene::new();
    let app = scene.app_with(
        "note",
        "startup: walk: 1.2.3 ms\n@wall 4\nstartup: first present 4.0 ms\n",
    );
    let mut out = Vec::new();
    let c = case("startup", "single-odd");
    let code = startup::run(
        &["2".into(), app],
        &scene.env(&c),
        &mut scene.host(&c),
        &mut out,
    );
    assert_eq!(code, 0, "a colon in the name, so 1.2.3 is never read");
}

/// **`\r\n`, a lone `\r` and `\n` each end a line**, as a text-mode pipe
/// reads them: three lines of a launch here share no `\n`.
#[test]
fn every_ending_ends_a_line() {
    let (out, code) = outcome("startup", "crlf");
    assert_eq!(code, 0);
    assert!(
        out.contains("startup: lone 2.0 ms\nstartup: after 3.0 ms\nstartup: more 3.5 ms\n"),
        "{out}"
    );
    let mut l = host::Lines::new(&b"a\r\nb\rc\nd\r"[..]);
    let mut got = Vec::new();
    while let Some(line) = l.next_line().expect("a line") {
        got.push(line);
    }
    assert_eq!(got, ["a\n", "b\n", "c\n", "d\n"]);
    let mut l = host::Lines::new(&b"tail"[..]);
    assert_eq!(
        l.next_line().expect("a line").as_deref(),
        Some("tail"),
        "no ending"
    );
    let mut l = host::Lines::new(&b"\xff\n"[..]);
    assert!(l.next_line().is_err(), "not UTF-8");
}

/// A `\r` a read ends on may be half of a `\r\n`: one reader, one byte a
/// read, still one ending.
#[test]
fn a_split_crlf_is_one_ending() {
    struct Dribble(&'static [u8]);
    impl std::io::Read for Dribble {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some((first, rest)) = self.0.split_first() else {
                return Ok(0);
            };
            buf[0] = *first;
            self.0 = rest;
            Ok(1)
        }
    }
    let mut l = host::Lines::new(Dribble(b"a\r\nb\n"));
    assert_eq!(l.next_line().expect("a line").as_deref(), Some("a\n"));
    assert_eq!(l.next_line().expect("a line").as_deref(), Some("b\n"));
    assert_eq!(l.next_line().expect("the end"), None);
}

/// A launch that never gets there says what it saw, in `repr`'s quoting.
#[test]
fn a_launch_that_never_reaches_the_stage_says_what_it_saw() {
    let (out, code) = outcome("startup", "never");
    assert_eq!(code, 1);
    assert!(
        out.contains(r#"'startup: it\'s "both" 2.0 ms', "startup: it's 3.0 ms""#),
        "{out}"
    );
    assert!(out.contains(r"'startup: tab\there 4.0 ms'"), "{out}");
    assert!(out.contains(r"'startup: back\\slash 5.0 ms'"), "{out}");
}

/// `BAUD_CMD`, `BAUD_GRID` and `BAUD_STARTUP`, as each launch has them.
#[test]
fn a_launch_has_the_environment_it_was_promised() {
    let (out, _) = outcome("startup", "env-default");
    assert!(
        out.contains("BAUD_CMD=sleep 5\n") && out.contains("BAUD_GRID=200x50\n"),
        "{out}"
    );
    assert!(out.contains("BAUD_STARTUP=1\n"), "{out}");
    let (out, _) = outcome("startup", "env-shell");
    assert!(
        out.contains("BAUD_CMD=unset\n"),
        "`shell` is no child at all: {out}"
    );
    let (out, _) = outcome("startup", "env-given");
    assert!(
        out.contains("BAUD_CMD=true\n") && out.contains("BAUD_GRID=80x24\n"),
        "{out}"
    );
    assert!(
        out.contains("BAUD_STARTUP=1\n"),
        "the switch is always on: {out}"
    );
}

/// The stage is names with `|` between them, as its usage says. The Python
/// handed it to `re`, which took a pattern too; none was ever documented.
#[test]
fn the_stage_named_is_a_name_not_a_pattern() {
    let scene = Scene::new();
    let c = Case {
        tool: "startup".into(),
        name: "pattern".into(),
        code: 1,
        env: vec![("BAUD_BENCH_STAGE".into(), "first.*".into())],
        args: vec!["2".into(), "{app:s-never}".into()],
    };
    assert_eq!(run(&scene, &c).1, 1);
    assert!(!startup::reaches(
        "startup: first present 4.0 ms",
        &["first.*"]
    ));
}

/// `BAUD_BENCH_STAGE` moves the wall.
#[test]
fn the_stage_named_is_the_one_the_wall_is_taken_to() {
    let (out, code) = outcome("startup", "stage-content");
    assert_eq!(code, 0);
    assert!(
        out.contains("round 1 arm base :     44.2 ms to the stage"),
        "{out}"
    );
    let (out, code) = outcome("startup", "stage-absent");
    assert_eq!(code, 1);
    assert!(out.contains("never reached the stage"), "{out}");
}

// startup-bench and shader-cache-bench: the statistics.

/// **The upper of the two middles**, which is not `statistics.median`.
#[test]
fn an_even_count_takes_the_upper_middle() {
    assert_eq!(host::upper_median(&[1.0, 2.0, 3.0, 4.0]), Some(3.0));
    assert_eq!(
        host::upper_median(&[4.0, 1.0, 3.0]),
        Some(3.0),
        "sorted first"
    );
    assert_eq!(host::upper_median(&[]), None);
    let (out, _) = outcome("startup", "single-even");
    // Six settled walls: 40.25 41.75 42 43.5 44 45.5.
    assert!(out.contains("median 43.5 ms, range [40.3-45.5]"), "{out}");
}

/// The first round is reported and is in no median: s-a's first wall is
/// 71.5, past every other.
#[test]
fn the_first_round_is_discarded() {
    let (out, _) = outcome("startup", "single-odd");
    assert!(
        out.contains("round 0 arm base :     71.5 ms to the stage  (discarded)"),
        "{out}"
    );
    assert!(out.contains("(2 settled launches)"), "{out}");
    assert!(!out.contains("-71.5]"), "{out}");
    let (out, _) = outcome("startup", "one-round");
    assert!(
        out.contains("(1 settled launches)") && out.contains("[71.5-71.5]"),
        "{out}"
    );
}

/// **A difference clears the A/A spread and the ranges part, or it is not a
/// result**, in either direction.
#[test]
fn a_result_is_past_the_spread_and_apart() {
    for (case, sign) in [("aa-result", "+"), ("aa-result-lower", "-")] {
        let (out, _) = outcome("startup", case);
        let line = out
            .lines()
            .find(|l| l.contains(": wall "))
            .expect("a wall line");
        assert!(line.contains(&format!(" wall {sign}")), "{line}");
        assert!(line.ends_with(": result)"), "{line}");
    }
    let (out, _) = outcome("startup", "aa-overlap");
    assert!(
        out.contains("inside the A/A spread -- not a result)"),
        "{out}"
    );
    let (out, _) = outcome("startup", "no-aa");
    assert!(!out.contains("(A/A"), "no A/A arm, no floor: {out}");
    // Past the spread by median, and one launch inside the base's range.
    let (out, _) = outcome("startup", "aa-past-spread-overlapping");
    let line = out
        .lines()
        .find(|l| l.contains(": wall "))
        .expect("a wall line");
    assert!(line.ends_with("not a result)"), "{line}");
    // Apart from the base, and inside what two launches of one program read.
    let (out, _) = outcome("startup", "aa-apart-inside-spread");
    let line = out
        .lines()
        .find(|l| l.contains(": wall "))
        .expect("a wall line");
    assert!(line.ends_with("not a result)"), "{line}");
}

/// A stage one arm lacks is named, and only the shared ones are compared.
#[test]
fn a_stage_only_one_arm_has_is_named() {
    let (out, _) = outcome("startup", "aa-overlap");
    assert!(
        out.contains("stages the arm no longer prints: dock icon\n"),
        "{out}"
    );
    assert!(out.contains("stages new to the arm: menu\n"), "{out}");
    assert!(
        out.contains("(A/A 0.2)"),
        "the A/A column is |aa - base| on the stage: {out}"
    );
}

/// **Two stages at one median come out by name.** The Python left them in
/// the order of a `set` of strings, which moves with `PYTHONHASHSEED`, so
/// this is the one line here with no golden to hold it to.
#[test]
fn stages_at_one_median_are_listed_by_name() {
    let scene = Scene::new();
    let app = scene.app_with(
        "tie",
        "startup: zeta 2.0 ms\nstartup: alpha 2.0 ms\nstartup: mid 2.0 ms\n@wall 4\nstartup: first present 4.0 ms\n",
    );
    let c = case("startup", "single-odd");
    let mut out = Vec::new();
    let code = startup::run(
        &["2".into(), app],
        &scene.env(&c),
        &mut scene.host(&c),
        &mut out,
    );
    assert_eq!(code, 0);
    let out = String::from_utf8(out).expect("UTF-8");
    let order: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("median      2.0"))
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(order, ["alpha", "mid", "zeta"], "{out}");
}

/// Each arm takes each place in turn.
#[test]
fn the_order_rotates_round_by_round() {
    assert_eq!(host::rotated(&[1, 2, 3], 0), [1, 2, 3]);
    assert_eq!(host::rotated(&[1, 2, 3], 1), [2, 3, 1]);
    assert_eq!(host::rotated(&[1, 2, 3], 4), [2, 3, 1]);
    let (out, _) = outcome("startup", "no-aa");
    let arms: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("round "))
        .filter_map(|l| l.split_whitespace().nth(3))
        .collect();
    assert_eq!(
        arms,
        ["base", "new", "new", "base", "base", "new", "new", "base"]
    );
}

/// Without a binary there is nothing to launch: a default would start
/// whichever app the checkout last built.
#[test]
fn the_binary_is_an_argument() {
    assert_eq!(outcome("startup", "no-arguments"), (String::new(), 1));
    assert_eq!(outcome("startup", "no-binary"), (String::new(), 1));
    // With no launch to make, a default binary would get as far as the
    // report's first line. The Python printed it.
    let c = case("startup", "no-binary");
    let scene = Scene::new();
    let mut out = Vec::new();
    let code = startup::run(&["0".into()], &scene.env(&c), &mut scene.host(&c), &mut out);
    assert_eq!(
        (out.as_slice(), code),
        (&b""[..], 1),
        "stopped at the usage"
    );
    assert_eq!(outcome("shader", "no-arguments"), (String::new(), 1));
    assert_eq!(outcome("shader", "no-binary"), (String::new(), 1));
    assert!(
        !startup::USAGE.contains("target/") && !shader_cache::USAGE.contains("target/"),
        "no usage names a build of the app"
    );
}

// shader-cache-bench: the parsers.

/// The `gpu build:` line, field by field; anything after the probe's value
/// is ignored, and a source is one word.
#[test]
fn the_build_line_gives_its_six_fields() {
    let line = "startup: gpu build: device 5.0 ms, library 3.5 (Library), pipelines 45.0, warm pass 2.0, probe 60.0";
    let want = (["5.0", "3.5", "45.0", "2.0", "60.0"], "Library");
    assert_eq!(shader_cache::build_line(line), Some(want));
    assert_eq!(
        shader_cache::build_line(&format!("{line} and after")),
        Some(want)
    );
    for broken in [
        line.replace("device 5.0", "device x"),
        line.replace("(Library)", "()"),
        line.replace("(Library)", "(Lib-rary)"),
        line.replace(" ms,", ","),
        line.replace("warm pass", "warm"),
        line.replace("probe 60.0", "probe"),
        line.replacen("startup: ", "", 1),
    ] {
        assert_eq!(shader_cache::build_line(&broken), None, "{broken}");
    }
}

/// The five, one at a time.
#[test]
fn the_pipelines_line_gives_its_five_fields() {
    let line =
        "startup: gpu pipelines: bg 1.5 ms, glyph 2.5, cell_bg 3.5, cell_glyph 4.5, cell_shape 5.5";
    let want = ["1.5", "2.5", "3.5", "4.5", "5.5"];
    assert_eq!(shader_cache::pipes_line(line), Some(want));
    assert_eq!(
        shader_cache::pipes_line(&format!("{line} and after")),
        Some(want)
    );
    for broken in [
        line.replace("bg 1.5", "bg x"),
        line.replace("cell_shape 5.5", "cell_shape"),
        line.replace("bg 1.5 ms", "bg 1.5"),
        line.replace("glyph 2.5, ", ""),
    ] {
        assert_eq!(shader_cache::pipes_line(&broken), None, "{broken}");
    }
}

/// Here the line keeps its `\n`, so a bare `first present` is one, and one
/// the stream ended before finishing is not.
#[test]
fn a_bare_first_present_needs_its_newline() {
    assert!(shader_cache::first_present("startup: first present\n"));
    assert!(!shader_cache::first_present("startup: first present"));
    assert_eq!(outcome("shader", "tails").1, 0);
    let (out, code) = outcome("shader", "no-final-newline");
    assert_eq!(code, 1);
    assert!(
        out.contains("no gpu build line, or no first present; stopping"),
        "{out}"
    );
}

/// A launch has the switch, the grid and the child, whatever the
/// environment said: the stand-in exits on anything else.
#[test]
fn a_gpu_launch_has_the_environment_it_was_promised() {
    assert_eq!(outcome("shader", "environment").1, 0);
}

/// Each way a launch comes up short says which.
#[test]
fn a_short_launch_says_which_line_it_lacks() {
    let (out, code) = outcome("shader", "no-gpu");
    assert_eq!(code, 1);
    assert!(
        out.contains("launch 0 cold: no gpu build line, or no first present; stopping"),
        "{out}"
    );
    let (out, code) = outcome("shader", "no-pipes");
    assert_eq!(code, 1);
    assert!(
        out.contains(
            "launch 0 cold: no gpu pipelines line: the five cannot be attributed; stopping"
        ),
        "{out}"
    );
}

// shader-cache-bench: the caches.

/// **The cold arm has no cache and the warm arm has it**: the stand-in
/// prints different numbers by whether the directory is there, so an arm
/// that moved nothing would read as warm.
#[test]
fn the_cold_arm_runs_without_the_caches() {
    let (out, _) = outcome("shader", "three");
    assert!(
        out.contains("library        6.0 [   5.0-   6.0]          32.5"),
        "{out}"
    );
    assert!(
        out.contains("the cold arm moved aside: com.apple.metal, com.apple.metalfe.\n"),
        "{out}"
    );
    // Only the cache named moves: this stand-in reads the front end's.
    let (out, _) = outcome("shader", "aside-metal-fe-stays");
    assert!(
        out.contains("+0.0 ms\nprobe"),
        "the front end stayed: {out}"
    );
    let (out, _) = outcome("shader", "aside-metalfe");
    assert!(
        out.contains("the cold arm moved aside: com.apple.metalfe.\n"),
        "{out}"
    );
    assert!(out.contains("+29.0 ms"), "the front end went: {out}");
}

/// Every way out leaves the caches as they were found.
#[test]
fn the_caches_are_back_on_every_way_out() {
    for c in cases("shader") {
        let setup = c
            .env
            .iter()
            .find(|(k, _)| k == "SETUP")
            .map(|(_, v)| v.as_str());
        if setup == Some("nocaches") {
            continue;
        }
        let scene = Scene::new();
        run(&scene, &c);
        for name in ["com.apple.metal", "com.apple.metalfe"] {
            assert!(
                scene.cache(name).join("marker").exists(),
                "{}: {name}",
                c.name
            );
            let aside = scene.cache(&format!("{name}.shader-cache-bench-aside"));
            assert!(!aside.exists(), "{}: {name} left aside", c.name);
        }
    }
}

/// **A leftover from an interrupted run goes back first**, over what is
/// there, so nothing is buried twice.
#[test]
fn a_leftover_goes_back_first() {
    let c = case("shader", "leftover");
    let scene = Scene::new();
    let (_, code) = run(&scene, &c);
    assert_eq!(code, 0);
    let kept = fs::read_to_string(scene.cache("com.apple.metal").join("marker")).expect("a marker");
    assert_eq!(kept, "older");
}

/// The directories it restores from and to, alone.
#[test]
fn restoring_replaces_what_the_cold_launch_wrote() {
    let scene = Scene::new();
    let root = scene.dir.join("root").display().to_string();
    let caches = shader_cache::Caches::of(&root, &["com.apple.metal"]);
    caches.move_aside().expect("aside");
    assert!(!scene.cache("com.apple.metal").exists());
    assert!(
        scene.cache("com.apple.metalfe").exists(),
        "it was not named"
    );
    fs::create_dir_all(scene.cache("com.apple.metal")).expect("the launch writes");
    fs::write(scene.cache("com.apple.metal/new"), "x").expect("the launch writes");
    caches.restore().expect("back");
    assert!(!scene.cache("com.apple.metal/new").exists());
    assert!(scene.cache("com.apple.metal/marker").exists());
    assert!(
        !scene
            .cache("com.apple.metal.shader-cache-bench-aside")
            .exists()
    );
    caches.restore().expect("twice is nothing");
    assert!(scene.cache("com.apple.metal/marker").exists());
}

/// **An interrupt puts the caches back and exits as one**, however far the
/// launch got.
#[test]
fn an_interrupt_puts_the_caches_back() {
    for after in 0..40 {
        let c = case("shader", "one");
        let scene = Scene::new();
        let mut host = scene.host(&c);
        host.interrupt_after = Some(after);
        let mut out = Vec::new();
        let args = scene.args(&c);
        let code = shader_cache::run(&args, &scene.env(&c), &mut host, &mut out);
        // Past the last look the run simply finished, which proves nothing.
        if host.fired == Fired::Yes {
            assert_eq!(code, 130, "after {after}");
        } else {
            assert_eq!(code, 0, "after {after}: never interrupted");
        }
        if after == 0 {
            assert!(out.is_empty(), "no launch was reported");
        }
        assert!(
            scene.cache("com.apple.metal/marker").exists(),
            "after {after}"
        );
        assert!(
            scene.cache("com.apple.metalfe/marker").exists(),
            "after {after}"
        );
    }
}

/// **`SIGINT`, `SIGTERM` and `SIGHUP` all reach the handler**: a signal left
/// out would end this process with the caches aside, and this test with it.
#[test]
fn each_way_of_being_stopped_is_caught() {
    host::catch_interrupt();
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: the handler installed above only stores to an atomic.
        assert_eq!(unsafe { libc::raise(signal) }, 0);
    }
    assert!(Live::new().interrupted());
}

/// An interrupt in the sleep after the last launch still stops the run, with
/// no report: what the launches measured was not all it was asked for.
#[test]
fn an_interrupt_after_the_last_launch_stops_the_report() {
    let c = case("shader", "one");
    let scene = Scene::new();
    let mut host = scene.host(&c);
    host.interrupt_in_pause = Some(2);
    let mut out = Vec::new();
    let args = scene.args(&c);
    let code = shader_cache::run(&args, &scene.env(&c), &mut host, &mut out);
    assert_eq!(code, 130);
    let out = String::from_utf8(out).expect("UTF-8");
    assert_eq!(out.lines().count(), 2, "two launches, no report: {out}");
}

/// **An arm that differs from itself is refused**, and a launch that is
/// discarded cannot make it differ.
#[test]
fn an_arm_of_two_programs_is_refused() {
    let (out, code) = outcome("shader", "mixed");
    assert_eq!(code, 1);
    assert!(out.contains("library from: Compiled, Library\n"), "{out}");
    let (out, code) = outcome("shader", "mixed-first-only");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("warm  library from: Library\n"), "{out}");
}

/// The second `gpu build:` line is the one kept, and a line past the first
/// present is never read.
#[test]
fn the_last_build_line_before_the_present_counts() {
    let (out, code) = outcome("shader", "twice");
    assert_eq!(code, 0);
    assert!(out.contains("library    9.5 ms (Compiled)"), "{out}");
    assert!(!out.contains("Other"), "{out}");
}

// idle-cpu: the statistics.

/// **The two middles' mean**, which is not the upper middle the other two
/// programs take.
#[test]
fn idle_cpu_takes_the_true_median() {
    assert_eq!(idle_cpu::true_median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
    assert_eq!(idle_cpu::true_median(&[1.0, 2.0, 3.0]), 2.0);
    let (out, _) = outcome("idle", "three-arms-even");
    assert!(
        out.contains("base           0.435% [  0.401-  0.490]"),
        "{out}"
    );
    let (out, _) = outcome("idle", "three-arms-odd");
    assert!(out.contains("(the aa arm's own spread)"), "{out}");
}

/// The floor is the last arm's own spread, and a result is apart, past the
/// floor and at least 5% of the base.
#[test]
fn a_result_needs_all_three() {
    let (out, _) = outcome("idle", "two-arms");
    assert!(out.ends_with("ranges disjoint -- result\n"), "{out}");
    let (out, _) = outcome("idle", "disjoint-lower");
    assert!(
        out.ends_with("ranges disjoint -- result\n"),
        "below the base: {out}"
    );
    let (out, _) = outcome("idle", "disjoint-small");
    assert!(
        out.ends_with("ranges disjoint -- not a result\n"),
        "under 5%: {out}"
    );
    let (out, _) = outcome("idle", "disjoint-big-under-floor");
    assert!(out.ends_with("-- not a result\n"), "under the floor: {out}");
    let (out, _) = outcome("idle", "three-arms-even");
    assert!(out.contains("ranges overlap -- not a result\n"), "{out}");
}

/// No settled round, no row; a base at zero has no ratio to print.
#[test]
fn nothing_settled_is_only_the_header() {
    let (out, code) = outcome("idle", "one-round");
    assert_eq!(code, 0);
    assert!(out.ends_with("a tick\n"), "{out}");
    let (out, code) = outcome("idle", "zero-base");
    assert_eq!(code, 1);
    assert!(
        out.ends_with("% of a core\n"),
        "stopped before the ratio: {out}"
    );
}

/// A process that is gone at either sample ends the run.
#[test]
fn a_process_gone_at_a_sample_ends_the_run() {
    let (out, code) = outcome("idle", "gone-in-settle");
    assert_eq!(
        (out.lines().count(), code),
        (2, 1),
        "two rounds, then gone: {out}"
    );
    let (out, code) = outcome("idle", "gone-in-window");
    assert_eq!(
        (out.lines().count(), code),
        (1, 1),
        "one round, then gone: {out}"
    );
}

/// **Its command line is argparse's**: exit 2 on bad usage, 0 on help, a
/// long option by any prefix only it begins with, help as soon as it is
/// reached.
#[test]
fn idle_cpu_reads_its_command_line_as_argparse_does() {
    assert_eq!(outcome("idle", "no-arm").1, 2);
    assert_eq!(outcome("idle", "prefix-ambiguous").1, 2);
    assert_eq!(outcome("idle", "prefix-h-ambiguous").1, 2);
    assert_eq!(outcome("idle", "prefix").1, 0);
    assert_eq!(
        outcome("idle", "help-prefix"),
        (idle_cpu::HELP.to_string(), 0)
    );
    assert_eq!(
        outcome("idle", "help-after-unknown"),
        (idle_cpu::HELP.to_string(), 0)
    );
    assert_eq!(outcome("idle", "help-after-bad-value").1, 2);
    assert_eq!(outcome("idle", "cmd-dash").1, 2);
    assert_eq!(outcome("idle", "cmd-dash-space").1, 0);
    assert_eq!(
        outcome("idle", "settle-negative").1,
        1,
        "a value, and sleep refuses it"
    );
}

/// **`=` is looked for before a space**, so `--cmd=sleep 600` is an option, and
/// the same order makes `--arm=a b` an option where a value was wanted. Only
/// what names no option is left to the space.
#[test]
fn a_space_is_read_after_the_option_is_looked_for() {
    for name in [
        "space-inline-cmd",
        "space-inline-prefix",
        "space-inline-rounds",
        "space-inline-arm",
        "space-inline-plain",
        "space-value-names-nothing",
    ] {
        assert_eq!(outcome("idle", name).1, 0, "{name}");
    }
    for name in [
        "space-value-names-option",
        "space-inline-unknown",
        "space-inline-aa",
    ] {
        assert_eq!(outcome("idle", name).1, 2, "{name}");
    }
}

/// **A negative value is argparse's pattern, not whatever `float` takes**:
/// `-1e5`, `-1.` and `-1_0` are numbers to `float` and options here, so the run
/// never starts.
#[test]
fn a_negative_value_is_the_pattern_and_not_float() {
    for name in [
        "negative-exponent",
        "negative-upper-exponent",
        "negative-trailing-point",
        "negative-point-exponent",
        "negative-underscore",
        "negative-settle-point",
        "negative-settle-exponent",
        "negative-rounds-underscore",
    ] {
        assert_eq!(outcome("idle", name).1, 2, "{name}");
    }
    for name in [
        "negative-hz-whole",
        "negative-hz-bare-point",
        "negative-hz-point",
    ] {
        assert_eq!(outcome("idle", name).1, 0, "{name}");
    }
    // The pattern's `$` also takes a closing newline, which a row cannot hold.
    let scene = Scene::new();
    let app = scene.app("idle");
    let c = case("idle", "zero-rounds");
    for (value, code) in [("-1\n", 0), ("-.5\n", 0), ("-1.\n", 2), ("-1\n\n", 2)] {
        let args = [
            "--arm",
            &format!("base={app}"),
            "--rounds",
            "0",
            "--hz",
            value,
        ]
        .map(str::to_string);
        let got = idle_cpu::run(&args, &scene.env(&c), &mut scene.host(&c), &mut Vec::new());
        assert_eq!(got, code, "{value:?}");
    }
}

/// **An ambiguous name stops the line where it stands**, where an unknown one
/// waits for the end and help can still come first.
#[test]
fn an_ambiguous_name_stops_before_help() {
    assert_eq!(outcome("idle", "ambiguous-before-help").1, 2);
    assert_eq!(outcome("idle", "ambiguous-inline-before-help").1, 2);
    assert_eq!(outcome("idle", "help-after-unknown").1, 0);
}

/// **`-h` opens a run of single-letter options**: what follows it is not read
/// before help is, unless it is an explicit argument, and anything that begins
/// `-h` is an option and so no value.
#[test]
fn what_follows_short_help_is_read_as_argparse_does() {
    assert_eq!(
        outcome("idle", "help-joined"),
        (idle_cpu::HELP.to_string(), 0)
    );
    assert_eq!(
        outcome("idle", "help-joined-space"),
        (idle_cpu::HELP.to_string(), 0)
    );
    assert_eq!(outcome("idle", "help-joined-equals").1, 2);
    assert_eq!(outcome("idle", "help-joined-dash").1, 2);
    assert_eq!(outcome("idle", "help-shaped-value").1, 2);
}

/// **Before an `=`, a lone `-` begins every option's name**, so `-=x` is an
/// ambiguous option and not one nothing knows: it stops the line, and with a
/// space in it is still no value.
#[test]
fn a_lone_dash_before_an_equals_is_ambiguous() {
    assert_eq!(outcome("idle", "dash-equals-before-help").1, 2);
    assert_eq!(outcome("idle", "dash-equals-bare-before-help").1, 2);
    assert_eq!(outcome("idle", "dash-equals-value").1, 2);
}

/// **A sleep too long for Python's clock is an error and not a panic**: it
/// stops the child and exits 1.
#[test]
fn a_sleep_past_the_clock_is_an_error() {
    assert_eq!(outcome("idle", "settle-too-long"), (String::new(), 1));
    assert_eq!(outcome("idle", "window-too-long"), (String::new(), 1));
}

/// **`--` is left over even with nothing after it.**
#[test]
fn a_lone_double_dash_is_left_over() {
    assert_eq!(outcome("idle", "double-dash-alone").1, 2);
}

/// Whether no process has this pid. A child this process has waited for has
/// none.
fn is_gone(pid: u32) -> bool {
    let pid = libc::pid_t::try_from(pid).expect("a pid");
    // SAFETY: signal 0 is only asked whether the process is there.
    unsafe { libc::kill(pid, 0) != 0 }
}

/// **An interrupt in either sleep stops the child and exits as one**, and the
/// copy `--aa` made goes with it.
#[test]
fn an_interrupt_in_idle_cpu_stops_the_child() {
    let c = case("idle", "one-round");
    for in_pause in [1, 2] {
        let scene = Scene::new();
        let mut host = scene.host(&c);
        host.interrupt_in_pause = Some(in_pause);
        let mut out = Vec::new();
        let code = idle_cpu::run(&scene.args(&c), &scene.env(&c), &mut host, &mut out);
        assert_eq!(code, 130, "in sleep {in_pause}");
        assert!(
            out.is_empty(),
            "no report: {}",
            String::from_utf8_lossy(&out)
        );
        match (in_pause, host.read_pid) {
            (1, None) => {}
            (2, Some(pid)) => assert!(is_gone(pid), "the child {pid} is still there"),
            (_, read) => panic!("in sleep {in_pause}, the last reading was of {read:?}"),
        }
        let left = fs::read_dir(&scene.dir)
            .expect("the scene")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("idle-aa-"))
            .count();
        assert_eq!(left, 0, "the copy stayed");
    }
}

/// **A signal sent to the driver alone reaches what it launched**, for each
/// signal that counts as an interrupt: it wakes from a long sleep, stops the
/// child and exits 130. In a process of its own, because it ends with the
/// process's one flag set.
#[test]
fn a_real_interrupt_ends_the_sleep_and_stops_the_child() {
    const DRIVER: &str = "BAUD_HOST_TEST_DRIVER_ARGS";
    if let Some(args) = std::env::var_os(DRIVER) {
        host::catch_interrupt();
        let text = args.to_string_lossy();
        let args: Vec<String> = text.split('\n').map(str::to_string).collect();
        let env: Vec<_> = std::env::vars_os().collect();
        let mut sink = std::io::sink();
        std::process::exit(idle_cpu::run(&args, &env, &mut Live::new(), &mut sink));
    }
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let scene = Scene::new();
        let app = scene.app_with("idle", "@run\n");
        let seen = scene.dir.join("pid");
        let cmd = format!("echo $PPID >{0}.new; mv {0}.new {0}", seen.display());
        let args = [
            "--arm",
            &format!("a={app}"),
            "--rounds",
            "1",
            "--settle",
            "300",
            "--window",
            "300",
            "--cmd",
            &cmd,
        ]
        .join("\n");
        let mut driver = Command::new(std::env::current_exe().expect("this test"))
            .args([
                "--exact",
                "a_real_interrupt_ends_the_sleep_and_stops_the_child",
                "--nocapture",
            ])
            .env(DRIVER, &args)
            .env("PATH", stand_in_path())
            .env("TMPDIR", &scene.dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the driver");
        let give_up = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let wait_out = |driver: &mut std::process::Child, what: &str| {
            assert!(
                std::time::Instant::now() < give_up,
                "signal {signal}: {what}: {:?}",
                driver.kill()
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        while !seen.exists() {
            wait_out(&mut driver, "the child never wrote its pid");
        }
        let child: u32 = fs::read_to_string(&seen)
            .expect("the pid")
            .trim()
            .parse()
            .expect("a number");
        let pid = libc::pid_t::try_from(driver.id()).expect("a pid");
        // SAFETY: `pid` is a child this test has not yet waited for.
        assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
        let status = loop {
            if let Some(status) = driver.try_wait().expect("the driver's status") {
                break status;
            }
            wait_out(&mut driver, "the driver slept on");
        };
        assert_eq!(status.code(), Some(130), "signal {signal}");
        assert!(
            is_gone(child),
            "signal {signal}: the child {child} is there"
        );
    }
}

/// **A real interrupt is caught whoever launched the driver**: the test
/// above, run again from this binary with `SIGHUP` blocked, as cargo-watch
/// hands its command, and `SIGINT` and `SIGQUIT` ignored, as a background
/// job of a non-interactive shell has them, passes, and ran; its drivers
/// start as it was started. The control: a shell under the same launch
/// survives its own `kill -HUP`, `kill -INT` and `kill -QUIT`, so the launch
/// is what it says. Red without the unblock in `host::catch_interrupt`: a
/// blocked `SIGHUP` stays pending and the driver sleeps on.
#[test]
fn a_real_interrupt_is_caught_whoever_launched_the_driver() {
    use std::os::unix::process::CommandExt;
    let launched = |cmd: &mut Command| {
        // SAFETY: between the fork and the exec the closure calls only
        // `sigemptyset`, `sigaddset`, `sigprocmask` and `signal`, on XSH
        // 2.4.3's table of async-signal-safe functions, over a set that
        // lives for the calls.
        unsafe {
            cmd.pre_exec(|| {
                let mut hup: libc::sigset_t = core::mem::zeroed();
                libc::sigemptyset(&raw mut hup);
                libc::sigaddset(&raw mut hup, libc::SIGHUP);
                libc::sigprocmask(libc::SIG_BLOCK, &raw const hup, core::ptr::null_mut());
                libc::signal(libc::SIGINT, libc::SIG_IGN);
                libc::signal(libc::SIGQUIT, libc::SIG_IGN);
                Ok(())
            })
        };
    };
    let mut control = Command::new("/bin/sh");
    control.args([
        "-c",
        "ulimit -c 0; kill -HUP $$; kill -INT $$; kill -QUIT $$; exit 7",
    ]);
    launched(&mut control);
    let survived = control.status().expect("sh");
    assert_eq!(survived.code(), Some(7), "the control: {survived:?}");
    let mut test = Command::new(std::env::current_exe().expect("this test"));
    test.args([
        "--exact",
        "a_real_interrupt_ends_the_sleep_and_stops_the_child",
    ]);
    launched(&mut test);
    let out = test.output().expect("the test");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{said}");
    assert!(said.contains("1 passed; 0 failed"), "it ran: {said}");
}

/// **The kernel's ticks are not nanoseconds**: the same CPU read through
/// `getrusage`. Read as nanoseconds it would be 24 times too small.
#[cfg(target_os = "macos")]
#[test]
fn the_cpu_reading_is_seconds() {
    fn getrusage_seconds() -> f64 {
        // SAFETY: `r` is a live value of the type the call writes.
        let r = unsafe {
            let mut r: libc::rusage = core::mem::zeroed();
            assert_eq!(libc::getrusage(libc::RUSAGE_SELF, &raw mut r), 0);
            r
        };
        let of = |t: libc::timeval| t.tv_sec as f64 + f64::from(t.tv_usec) / 1e6;
        of(r.ru_utime) + of(r.ru_stime)
    }
    let pid = std::process::id();
    let c0 = idle_cpu::cpu_seconds(pid).expect("this process");
    let g0 = getrusage_seconds();
    let mut x = 1u64;
    while getrusage_seconds() - g0 < 0.1 {
        for _ in 0..10_000 {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
        }
    }
    let spent = getrusage_seconds() - g0;
    let read = idle_cpu::cpu_seconds(pid).expect("this process") - c0;
    assert!(
        read > spent * 0.5 && read < spent * 2.0,
        "proc_pid_rusage read {read} s where getrusage read {spent} s"
    );
}

/// The child the row names is the one the launch has, whatever the
/// environment said.
#[test]
fn idle_cpu_names_the_child() {
    let parent = [
        ("BAUD_CMD".into(), "other".into()),
        ("KEPT".into(), "1".into()),
    ];
    let got = idle_cpu::child_env(&parent, "sleep 9");
    assert_eq!(host::var(&got, "BAUD_CMD").as_deref(), Some("sleep 9"));
    assert_eq!(host::var(&got, "KEPT").as_deref(), Some("1"));
    assert_eq!(got.iter().filter(|(k, _)| k == "BAUD_CMD").count(), 1);
}

/// A process that is gone reads as nothing.
#[cfg(target_os = "macos")]
#[test]
fn a_gone_process_has_no_reading() {
    let mut child = Command::new("/usr/bin/true").spawn().expect("true");
    let pid = child.id();
    child.wait().expect("it ended");
    assert_eq!(idle_cpu::cpu_seconds(pid), None);
}

// dock-launch-check.

/// `startup: locale: <name> in <ms> ms`, and nothing looser.
#[test]
fn the_locale_note_is_one_shape() {
    let n = dock_check::note;
    assert_eq!(
        n("startup: locale: en_US.UTF-8 in 0.4 ms"),
        Some(("en_US.UTF-8", "0.4"))
    );
    assert_eq!(
        n("startup: locale: q in . ms"),
        Some(("q", ".")),
        "digits and dots, unread"
    );
    assert_eq!(n("startup: locale: x in 1 ms and more"), None);
    assert_eq!(n("startup: locale: y z in 1 ms"), None);
    assert_eq!(n("startup: locale:  in 1 ms"), None);
    assert_eq!(n("startup: locale: x in  ms"), None);
    assert_eq!(n("startup: locale: x in x ms"), None);
}

/// **Each arm fails on what it is there to catch.**
#[cfg(target_os = "macos")]
#[test]
fn each_arm_of_the_dock_check_can_fail() {
    let (out, code) = outcome("dock", "good");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.matches("ok  ").count(), 3, "{out}");
    let (out, code) = outcome("dock", "no-read");
    assert_eq!(code, 1);
    assert!(
        out.contains("FAIL dock") && out.contains("FAIL empty"),
        "{out}"
    );
    assert!(
        out.contains("ok   control"),
        "the control passes without a read: {out}"
    );
    let (out, code) = outcome("dock", "note-always");
    assert_eq!(code, 1);
    assert!(
        out.contains("FAIL control"),
        "a note where none was asked for: {out}"
    );
    let (out, code) = outcome("dock", "not-utf8-name");
    assert_eq!(code, 1);
    assert!(
        out.contains("FAIL dock") && out.contains("ok   control"),
        "{out}"
    );
    assert!(out.contains("LANG='en_US.ISO8859-1'"), "{out}");
    let (out, code) = outcome("dock", "note-names-another");
    assert_eq!(code, 1);
    assert!(
        out.contains("FAIL dock") && out.contains("('de_DE.UTF-8', '0.2')"),
        "{out}"
    );
    let (out, code) = outcome("dock", "name-without-the-suffix");
    assert_eq!(code, 1);
    assert!(
        out.contains("FAIL dock     LANG='UTF-8' wc -m='1'"),
        "the name alone fails: {out}"
    );
    let (out, code) = outcome("dock", "silent");
    assert_eq!(code, 1);
    assert_eq!(out.matches("FAIL").count(), 3, "{out}");
}

/// An empty `LANG` is not an unset one: a stand-in that reads only the
/// second fails the `empty` arm alone, so that arm has it set and empty.
/// launchd's environment is what the launch has, and nothing else came.
#[cfg(target_os = "macos")]
#[test]
fn the_dock_check_launches_with_launchds_environment() {
    let (out, _) = outcome("dock", "empty-is-not-unset");
    assert!(
        out.contains("ok   dock") && out.contains("FAIL empty"),
        "{out}"
    );
    let (out, _) = outcome("dock", "environment");
    assert!(out.contains("('/bin/zsh', '0.1'), ('/usr/bin:/bin:/usr/sbin:/sbin', '0.1'), ('80x24', '0.1'), ('1', '0.1'), ('unset', '0.1'), ('unset', '0.1')"), "{out}");
}

/// Without a binary it runs nothing.
#[test]
fn the_dock_check_needs_a_program() {
    assert_eq!(outcome("dock", "no-arguments"), (String::new(), 1));
    assert_eq!(outcome("dock", "absent"), (String::new(), 1));
    assert_eq!(outcome("dock", "not-executable"), (String::new(), 1));
}

// vt-reply.

/// `row;col` picks the reply out from the ones a recording asked for, and
/// what came with it is printed.
#[test]
fn the_reply_is_the_one_asked_for() {
    assert_eq!(
        outcome("vt", "reply").0,
        "1b5b31333b3337 52\n".replace(' ', "")
    );
    assert_eq!(
        outcome("vt", "reply-after-others").0,
        "1b5b313b31521b5b3f36631b5b31333b33375274 61696c\n".replace(' ', "")
    );
    assert_eq!(outcome("vt", "other-position").0, "1b5b313b3152\n");
    assert_eq!(outcome("vt", "nothing").0, "none\n");
    assert_eq!(outcome("vt", "high-bytes").0, "ff8052\n");
    assert_eq!(outcome("vt", "timeout-word"), (String::new(), 1));
}

/// **With fewer than a row and a column any `R` ends it**, and the first
/// read's 256 bytes are all it took.
#[test]
fn without_a_position_any_r_ends_it() {
    assert_eq!(outcome("vt", "default-want").0, "6162526364\n");
    assert_eq!(outcome("vt", "two-arguments").0, "1b5b31333b333752\n");
    let (out, _) = outcome("vt", "first-chunk-stops");
    assert_eq!(out.len(), 256 * 2 + 1, "{out}");
    // With both, an `R` is not the reply.
    let (out, _) = outcome("vt", "an-r-before-the-reply");
    assert!(out.ends_with("1b5b31333b333752\n"), "{out}");
}

/// **The reply is searched for in everything read**: here it straddles the
/// first read's 256 bytes, and a search of the last read alone would miss it.
#[test]
fn a_reply_across_two_reads_is_found() {
    let (out, _) = outcome("vt", "across-the-chunk");
    assert!(out.ends_with("1b5b31333b3337527461696c\n"), "{out}");
    let (out, _) = outcome("vt", "past-the-chunk");
    assert!(out.ends_with("1b5b31333b333752\n"), "{out}");
    let (out, _) = outcome("vt", "stops-at-the-chunk-it-found");
    assert_eq!(
        out.len(),
        512 * 2 + 1,
        "the reply is in the second read: {out}"
    );
}

/// **On a real terminal**, in raw mode as `scripts/vt-child.sh` leaves it,
/// the reply arriving in two writes: the first half is read and not enough,
/// then the rest comes.
#[test]
fn a_reply_on_a_tty_in_two_writes_is_found() {
    let (mut master, mut slave) = (0, 0);
    // SAFETY: both out-pointers are live; the rest may be null. `t` is
    // written by `tcgetattr` before it is changed and read.
    unsafe {
        assert_eq!(
            libc::openpty(
                &raw mut master,
                &raw mut slave,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            ),
            0
        );
        let mut t: libc::termios = core::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave, &raw mut t), 0);
        libc::cfmakeraw(&raw mut t);
        assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &raw const t), 0);
    }

    /// Sends the second half on the reader's third look at the clock, which
    /// comes after at least one wait has found only the first half.
    struct SecondHalf {
        master: i32,
        looks: Calls,
        start: std::time::Instant,
    }
    impl Host for SecondHalf {
        fn now(&mut self) -> f64 {
            0.0
        }
        fn wall(&mut self) -> f64 {
            if self.looks.next() == 2 {
                let rest = b";37R";
                // SAFETY: `rest` is a live buffer of the length passed.
                let n = unsafe { libc::write(self.master, rest.as_ptr().cast(), rest.len()) };
                assert_eq!(n, 4);
            }
            self.start.elapsed().as_secs_f64()
        }
        fn pause(&mut self, _: f64) {}
        fn cpu_seconds(&mut self, _: u32) -> Option<f64> {
            None
        }
        fn cache_root(&mut self) -> Result<String, String> {
            Err(String::new())
        }
        fn interrupted(&mut self) -> bool {
            false
        }
    }
    let first = b"\x1b[13";
    // SAFETY: `first` is a live buffer of the length passed.
    assert_eq!(
        unsafe { libc::write(master, first.as_ptr().cast(), first.len()) },
        4
    );
    let mut host = SecondHalf {
        master,
        looks: Calls::default(),
        start: std::time::Instant::now(),
    };
    let mut out = Vec::new();
    let args = ["30".to_string(), "13".to_string(), "37".to_string()];
    let code = vt_reply::run(&args, slave, &mut host, &mut out);
    // SAFETY: both are this test's, and nothing else closes them.
    unsafe {
        libc::close(slave);
        libc::close(master);
    }
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8(out).expect("UTF-8"), "1b5b31333b333752\n");
    assert!(host.looks.0 >= 3, "the second half was sent");
}

// The Python values.

/// `repr`, `int` and `float`, on what these programs meet.
#[test]
fn python_s_values_read_and_print_as_python_s() {
    assert_eq!(py::repr_str("plain"), "'plain'");
    assert_eq!(py::repr_str("it's"), "\"it's\"");
    assert_eq!(py::repr_str("it's \"x\""), r#"'it\'s "x"'"#);
    assert_eq!(
        py::repr_str("a\tb\\c\u{1}\u{e9}\u{a0}"),
        "'a\\tb\\\\c\\x01\u{e9}\\xa0'"
    );
    assert_eq!(
        py::repr_list(&["a".to_string(), "b".to_string()]),
        "['a', 'b']"
    );
    assert_eq!(py::repr_list(&[]), "[]");
    assert_eq!(py::int(" +1_0 "), Some(10));
    assert_eq!(py::int("-3"), Some(-3));
    for not in ["", "1.5", "_1", "1_", "1__0", "x", "0x10", "+"] {
        assert_eq!(py::int(not), None, "{not:?}");
    }
    assert_eq!(py::float(" 1_0.5e1 "), Some(105.0));
    assert_eq!(py::float("1."), Some(1.0));
    assert_eq!(py::float(".5"), Some(0.5));
    assert!(py::float("nan").is_some_and(f64::is_nan));
    assert_eq!(py::float("-inf"), Some(f64::NEG_INFINITY));
    for not in ["", ".", "1.2.3", "_1.0", "1__0.0", "1_.0", "x"] {
        assert_eq!(py::float(not), None, "{not:?}");
    }
    assert_eq!(
        py::splitlines("a\nb\r\nc\u{b}d\u{2028}e\n"),
        ["a", "b", "c", "d", "e"]
    );
    assert_eq!(py::abspath("/a/./b/../c"), "/a/c");
    assert!(Path::new(&py::abspath("x")).is_absolute());
}
