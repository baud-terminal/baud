//! **A measurement does not share the machine with a warm build.**
//! `scripts/warm.sh` keeps the release test build warm while you work:
//! `warm-run` starts a watcher, and the watcher a build at each save.
//! Every bench and timing example calls [`refuse_while_warm`] before it
//! measures anything. Unpinned, it refuses while a watcher or a build of one
//! is running and that checkout has not been paused (`warm-pause`), and on
//! Linux beside any process of a build, paused or not: one a pause has not
//! stopped is still on the CPU.
//!
//! **A measurement pinned to some CPUs is judged on those alone** ([`on_own_cores`]).
//! On Linux, when this process's affinity leaves out an online CPU, its
//! cores are that affinity and each one's SMT siblings, which share a core
//! with it. It refuses only when one of them is at least `BAUD_QUIET_BUSY`
//! percent busy (default 5) over `BAUD_QUIET_SAMPLE` ms of /proc/stat
//! (default 500). A measurement that draws on the GPU calls
//! [`refuse_while_warm_on_gpu`], which also refuses when a GPU's
//! `gpu_busy_percent` reads at least `BAUD_QUIET_GPU_BUSY` (default 75) on
//! both of two reads 100 ms apart. A watcher or a build on other CPUs is
//! not load on its cores, so neither is asked about. A GPU with no such
//! file is not checked, and one whose file cannot be read refuses; so does
//! a CPU file absent or unreadable.
//!
//! **A check at the start cannot see load that arrives mid-run**, so
//! [`quiet_begin`] and [`Quiet::end`] bracket a timed run. Pinned, a run is
//! void when others' load on its own cores (their busy ticks less this
//! process's own CPU time), or its siblings' load, reached
//! `BAUD_QUIET_BUSY`, or, for a run that draws, the GPU reached its line.
//! `end` prints the conditions to stderr on one `quiet:` line, with the rest
//! of the machine's busy beside them, or a `void:` line naming why. An
//! unpinned run's conditions are recorded and never void it.
//!
//! An idle watcher counts, since the next save starts a build halfway
//! through the measurement. It is found on the whole machine, not in one
//! checkout: a timing A/B runs binaries built in a scratch copy of the tree,
//! which has no `.devenv/` of its own. The watcher and each build carry the
//! words `<root>/scripts/warm.sh build` in their command lines, and `<root>`
//! is where the pause is read.
//!
//! The watchers `devenv up` runs now are bacon and cargo-watch; `warm` is
//! started by `warm-run` alone. They are read through `BAUD_WATCH`, their
//! folder, which holds `.paused` while they are paused ([`watch_refusal`]).
//!
//! `bench-others/multibench` is a workspace of its own and links nothing
//! from this one, so it takes this file by `#[path]`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The pause, under a checkout's root.
pub const PAUSE: &str = ".devenv/state/warm.pause";

/// The variable every process of a warm build carries, with its checkout's
/// root for a value (`scripts/warm.sh`).
pub const TOKEN: &str = "BAUD_WARM_BUILD";

/// A process id, as the kernel numbers processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub u32);

impl std::fmt::Display for Pid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for Pid {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Pid, Self::Err> {
        s.parse().map(Pid)
    }
}

/// A watcher or a build, and the checkout it names.
#[derive(Debug, PartialEq, Eq)]
pub struct Warm {
    /// The process id.
    pub pid: Pid,
    /// `None` when the script's path is not absolute, so no pause can be
    /// read for it.
    pub root: Option<PathBuf>,
}

/// The warm processes in a listing of `<pid> <command line>` lines that no
/// pause covers. A line is one when two of its words are `scripts/warm.sh`,
/// by any path, and then `build`; the path's folder above `scripts/` is the
/// root, and `paused` is asked about it only when it is absolute.
pub fn unpaused(listing: &str, paused: impl Fn(&Path) -> bool) -> Vec<Warm> {
    // What is left of a word once `scripts/warm.sh` is taken off its end: the
    // root with its last slash, or nothing for the relative spelling.
    fn before_script(word: &str) -> Option<&str> {
        word.strip_suffix("scripts/warm.sh")
            .filter(|rest| rest.is_empty() || rest.ends_with('/'))
    }
    listing
        .lines()
        .filter_map(|line| {
            let (pid, args) = line.trim_start().split_once(char::is_whitespace)?;
            let pid: Pid = pid.parse().ok()?;
            let words: Vec<&str> = args.split_whitespace().collect();
            let rest = words
                .windows(2)
                .find_map(|w| before_script(w[0]).filter(|_| w[1] == "build"))?;
            let root = rest
                .strip_suffix('/')
                .filter(|r| r.starts_with('/'))
                .map(PathBuf::from);
            match &root {
                Some(r) if paused(r) => None,
                _ => Some(Warm { pid, root }),
            }
        })
        .collect()
}

/// This machine's processes, one `<pid> <command line>` a line. Linux reads
/// `/proc`, which a container without `ps` still has; elsewhere `/bin/ps`.
pub fn listing() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let mut out = String::new();
        let dir = std::fs::read_dir("/proc").map_err(|e| format!("/proc: {e}"))?;
        for entry in dir.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str() else { continue };
            if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            // A process that ended since the directory was read is skipped.
            let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            let args = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            out.push_str(pid);
            out.push(' ');
            out.push_str(args.trim_end());
            out.push('\n');
        }
        Ok(out)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let out = std::process::Command::new("/bin/ps")
            .args(["-A", "-o", "pid=", "-o", "args="])
            .output()
            .map_err(|e| format!("/bin/ps: {e}"))?;
        if !out.status.success() {
            return Err(format!("/bin/ps: {}", out.status));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// The root a process's environment names in [`TOKEN`], given the environment
/// as `/proc/<pid>/environ` spells it: `NAME=value` entries, each ended by a
/// NUL.
pub fn build_root(environ: &[u8]) -> Option<PathBuf> {
    environ.split(|&b| b == 0).find_map(|entry| {
        let value = entry.strip_prefix(TOKEN.as_bytes())?.strip_prefix(b"=")?;
        Some(PathBuf::from(String::from_utf8_lossy(value).into_owned()))
    })
}

/// Every process of a warm build that is running, paused or not, by
/// [`TOKEN`] in its environment. Linux alone shows another process's
/// environment, in `/proc`; elsewhere this finds none, and the command lines
/// [`unpaused`] reads are what there is.
pub fn building() -> Vec<Warm> {
    environs()
        .into_iter()
        .filter_map(|(pid, environ)| {
            build_root(&environ).map(|root| Warm {
                pid,
                root: Some(root),
            })
        })
        .collect()
}

/// Each process's id and environment, where they can be read.
fn environs() -> Vec<(Pid, Vec<u8>)> {
    #[cfg(target_os = "linux")]
    {
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|entry| {
                let pid: Pid = entry.file_name().to_str()?.parse().ok()?;
                // Another user's, or one that has ended, is not read.
                let environ = std::fs::read(entry.path().join("environ")).ok()?;
                Some((pid, environ))
            })
            .collect()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// The variable naming the watchers' folder: bacon's and cargo-watch's
/// (devenv.nix `processes`), where `watch-pause.sh on` writes `.paused`.
pub const WATCH: &str = "BAUD_WATCH";

/// Why a measurement must not start beside the watchers, or `None`. They
/// are paused only while their folder holds `.paused`, which
/// `watch-pause.sh on` writes once it has stopped them. An idle watcher
/// counts, with no job running, since the next save starts one halfway
/// through the measurement: so the test is the pause, never a live pid.
/// With no folder named (`BAUD_WATCH` unset: another machine, CI) there is
/// nothing to read.
pub fn watch_refusal(watch: Option<&Path>) -> Option<String> {
    let dir = watch?;
    if dir.join(".paused").exists() {
        return None;
    }
    Some(format!(
        "the watchers (bacon, cargo-watch) are not paused, and a measurement must not share the CPU with them: \
         {} holds no .paused; run `watch-pause.sh on` and start again",
        dir.display()
    ))
}

/// Why a measurement must not start now, or `None`. Pinned on Linux, its
/// own cores and the GPU decide ([`on_own_cores`]); otherwise the watchers
/// and warm builds do. A machine whose processes or cores cannot be read is
/// a reason: what cannot be seen is not known to be quiet.
pub fn refusal() -> Option<String> {
    refusal_on(Load::Cores)
}

/// [`refusal`], with `load` saying whether a pinned measurement's GPU is
/// read as well as its cores.
pub fn refusal_on(load: Load) -> Option<String> {
    if cfg!(target_os = "linux") {
        match on_own_cores(&Host, load) {
            Verdict::Quiet => return None,
            Verdict::Refused(why) => return Some(why),
            Verdict::Unpinned => {}
            Verdict::Counting => {
                return Some(format!(
                    "{MEASURES}=counts reads no load, and this measurement gives figures on the clock: \
                     unset it, or set it to `time`"
                ));
            }
        }
    }
    let named = std::env::var_os(WATCH).map(PathBuf::from);
    if let Some(why) = watch_refusal(named.as_deref()) {
        return Some(why);
    }
    let listing = match listing() {
        Ok(listing) => listing,
        Err(e) => {
            return Some(format!(
                "whether a warm build is running cannot be read ({e})"
            ));
        }
    };
    let warm = unpaused(&listing, |root| root.join(PAUSE).exists());
    let building = building();
    if warm.is_empty() && building.is_empty() {
        return None;
    }
    let mut why = String::from(
        "a warm build or its watcher is running, and a measurement must not share the CPU with it:",
    );
    for w in &building {
        if let Some(root) = &w.root {
            why.push_str(&format!(
                "\n  pid {}, a build in {}, which runs paused or not",
                w.pid,
                root.display()
            ));
        }
    }
    for w in &warm {
        match &w.root {
            Some(root) => why.push_str(&format!("\n  pid {} in {}", w.pid, root.display())),
            None => why.push_str(&format!(
                "\n  pid {}, whose checkout its command line does not name",
                w.pid
            )),
        }
    }
    why.push_str(
        "\nrun `warm-pause` in that checkout's devenv shell, or \
         `bash <checkout>/scripts/warm.sh pause`, and start again",
    );
    Some(why)
}

/// Exit before measuring when [`refusal`] has a reason.
pub fn refuse_while_warm() {
    exit_on(refusal());
}

/// Exit before measuring when [`refusal_on`] has a reason with the GPU
/// read: the guard of a measurement that draws on the GPU. Unpinned, it is
/// [`refuse_while_warm`].
pub fn refuse_while_warm_on_gpu() {
    exit_on(refusal_on(Load::CoresAndGpu));
}

/// Exit when there is a reason, naming it.
fn exit_on(refusal: Option<String>) {
    if let Some(why) = refusal {
        eprintln!("refused: {why}");
        std::process::exit(1);
    }
}

/// What a pinned measurement's quiet is read on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Load {
    /// Its cores and their SMT siblings.
    Cores,
    /// Its cores and every GPU: a measurement that draws.
    CoresAndGpu,
}

/// The kernel's file of each CPU's ticks.
pub const STAT: &str = "/proc/stat";

/// This process's status, whose `Cpus_allowed_list` is its affinity.
pub const STATUS: &str = "/proc/self/status";

/// The online CPUs.
pub const ONLINE: &str = "/sys/devices/system/cpu/online";

/// The DRM class folder, one `card<N>` in it for each GPU.
pub const DRM: &str = "/sys/class/drm";

/// The share of a core's sample busy at which a pinned measurement refuses.
pub const QUIET_BUSY: &str = "BAUD_QUIET_BUSY";

/// How long the cores are sampled, in milliseconds.
pub const QUIET_SAMPLE: &str = "BAUD_QUIET_SAMPLE";

/// The GPU busy percent at which a pinned measurement refuses.
pub const QUIET_GPU_BUSY: &str = "BAUD_QUIET_GPU_BUSY";

/// The GPU line when [`QUIET_GPU_BUSY`] is unset. It sits above what the
/// compositor's idle drawing reads: sampled at 10 Hz for 180 s, the lower of
/// two reads 100 ms apart was 58 at most and 24 at the 99th percentile, and
/// the line is 17 above that highest value.
pub const QUIET_GPU_BUSY_DEFAULT: BusyPct = BusyPct(75.0);

/// The time between the GPU's two reads.
pub const GPU_GAP: Duration = Duration::from_millis(100);

/// A CPU, as the kernel numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cpu(pub u32);

impl std::fmt::Display for Cpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cpu{}", self.0)
    }
}

/// A set of CPUs, written as the kernel writes a CPU list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoreSet(pub BTreeSet<Cpu>);

impl FromIterator<Cpu> for CoreSet {
    fn from_iter<I: IntoIterator<Item = Cpu>>(iter: I) -> CoreSet {
        CoreSet(iter.into_iter().collect())
    }
}

impl std::fmt::Display for CoreSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut cpus = self.0.iter().map(|c| c.0).peekable();
        let mut sep = "";
        while let Some(first) = cpus.next() {
            let mut last = first;
            while cpus.peek() == Some(&(last + 1)) {
                last += 1;
                cpus.next();
            }
            if first == last {
                write!(f, "{sep}{first}")?;
            } else {
                write!(f, "{sep}{first}-{last}")?;
            }
            sep = ",";
        }
        Ok(())
    }
}

/// A share of time busy, in percent.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct BusyPct(pub f64);

impl std::fmt::Display for BusyPct {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.0}%", self.0)
    }
}

/// A core and how busy it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Busy {
    /// The core.
    pub cpu: Cpu,
    /// Its share of the sample busy.
    pub pct: BusyPct,
}

/// What reading a file gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// There is no such file.
    Absent,
    /// It exists and could not be read, and why.
    Unreadable(String),
    /// Its text.
    Text(String),
}

/// Where a measurement may run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// On these online CPUs, and not on every one.
    Pinned(CoreSet),
    /// On every online CPU.
    Unpinned,
}

/// What a measurement measures: `time`, the default when unset, or
/// `counts`.
pub const MEASURES: &str = "BAUD_MEASURES";

/// What a measurement measures, which decides whether load is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measures {
    /// Time, which load on its cores or the GPU changes.
    Time,
    /// Instruction and cache counts under a simulator, which load does not
    /// change: a pinned counting run reads no load, and gives no figure on
    /// the clock.
    Counts,
}

impl Measures {
    /// `line` when this measures time; `None` when it counts, whose clock
    /// no load check guards.
    pub fn figure(self, line: impl FnOnce() -> String) -> Option<String> {
        match self {
            Measures::Time => Some(line()),
            Measures::Counts => None,
        }
    }
}

/// What [`MEASURES`] names on `m`. Any value other than `time` or `counts`
/// is an error naming it.
pub fn measures(m: &impl Machine) -> Result<Measures, String> {
    match m.var(MEASURES).as_deref() {
        None | Some("time") => Ok(Measures::Time),
        Some("counts") => Ok(Measures::Counts),
        Some(v) => Err(format!("{MEASURES}: `{v}` is neither `time` nor `counts`")),
    }
}

/// [`measures`] on this machine, exiting when [`MEASURES`] names neither.
// `bench-others/multibench` takes this file by `#[path]` and only times.
#[allow(dead_code)]
pub fn measures_or_refuse() -> Measures {
    match measures(&Host) {
        Ok(measures) => measures,
        Err(why) => {
            eprintln!("refused: {why}");
            std::process::exit(1);
        }
    }
}

/// The guard of a measurement that counts: pinned on Linux it reads no
/// load, and otherwise it is [`refuse_while_warm`]. Each variable of
/// `asks_for_time` that is set asks for a figure on the clock, and refuses.
// `bench-others/multibench` takes this file by `#[path]` and only times.
#[allow(dead_code)]
pub fn refuse_while_counting(asks_for_time: &[&str]) {
    if measures_or_refuse() == Measures::Time {
        refuse_while_warm();
        return;
    }
    for name in asks_for_time {
        if std::env::var_os(name).is_some() {
            eprintln!(
                "refused: {MEASURES}=counts gives no figure on the clock, and {name} asks for one"
            );
            std::process::exit(1);
        }
    }
    if cfg!(target_os = "linux") && on_own_cores(&Host, Load::Cores) == Verdict::Counting {
        return;
    }
    refuse_while_warm();
}

/// What a measurement's own cores say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It may run on every CPU, so its cores cannot tell its load from
    /// another's.
    Unpinned,
    /// Pinned, and its cores and the GPU are quiet.
    Quiet,
    /// Pinned, and why it must not start.
    Refused(String),
    /// Pinned, and counting: no load was read.
    Counting,
}

/// The files a verdict reads, and the clock it sleeps on: [`Host`] on this
/// machine, text in the tests.
pub trait Machine {
    /// A file's text.
    fn read(&self, path: &Path) -> Read;
    /// The names in [`DRM`], one a line.
    fn drm(&self) -> Read;
    /// An environment variable.
    fn var(&self, name: &str) -> Option<String>;
    /// Sleep for `how_long`.
    fn sleep(&self, how_long: Duration);
}

/// This machine.
pub struct Host;

impl Machine for Host {
    fn read(&self, path: &Path) -> Read {
        match std::fs::read_to_string(path) {
            Ok(text) => Read::Text(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Read::Absent,
            Err(e) => Read::Unreadable(e.to_string()),
        }
    }

    fn drm(&self) -> Read {
        let dir = match std::fs::read_dir(DRM) {
            Ok(dir) => dir,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Read::Absent,
            Err(e) => return Read::Unreadable(e.to_string()),
        };
        let mut names = String::new();
        for entry in dir {
            match entry {
                Ok(entry) => {
                    names.push_str(&entry.file_name().to_string_lossy());
                    names.push('\n');
                }
                Err(e) => return Read::Unreadable(e.to_string()),
            }
        }
        Read::Text(names)
    }

    fn var(&self, name: &str) -> Option<String> {
        std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
    }

    fn sleep(&self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

/// The CPUs a list names, as the kernel writes one: `0-3,8,10-11`.
pub fn cpu_list(text: &str) -> Result<CoreSet, String> {
    let text = text.trim();
    let bad = || format!("`{text}` is not a CPU list");
    if text.is_empty() {
        return Err(bad());
    }
    let mut set = BTreeSet::new();
    for part in text.split(',') {
        let (first, last) = part.split_once('-').unwrap_or((part, part));
        let first: u32 = first.parse().map_err(|_| bad())?;
        let last: u32 = last.parse().map_err(|_| bad())?;
        if first > last {
            return Err(bad());
        }
        set.extend((first..=last).map(Cpu));
    }
    Ok(CoreSet(set))
}

/// The affinity a status file ([`STATUS`]) names in its `Cpus_allowed_list`
/// line.
pub fn allowed(status: &str) -> Result<CoreSet, String> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Cpus_allowed_list:"))
        .ok_or_else(|| String::from("no Cpus_allowed_list line"))
        .and_then(cpu_list)
}

/// Pinned when the affinity leaves out an online CPU, to the online CPUs it
/// allows.
pub fn placement(allowed: &CoreSet, online: &CoreSet) -> Placement {
    if online.0.is_subset(&allowed.0) {
        Placement::Unpinned
    } else {
        Placement::Pinned(online.0.intersection(&allowed.0).copied().collect())
    }
}

/// Where a CPU's SMT siblings are listed, itself among them.
pub fn siblings_path(cpu: Cpu) -> PathBuf {
    PathBuf::from(format!(
        "/sys/devices/system/cpu/{cpu}/topology/thread_siblings_list"
    ))
}

/// `own` and each one's SMT siblings, read through `topology`. A topology
/// file absent or unreadable is an error naming it.
pub fn siblings(own: &CoreSet, topology: impl Fn(Cpu) -> Read) -> Result<CoreSet, String> {
    let mut all = own.0.clone();
    for &cpu in &own.0 {
        let path = siblings_path(cpu);
        let text = text(topology(cpu), &path)?;
        let named = cpu_list(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        all.extend(named.0);
    }
    Ok(CoreSet(all))
}

/// A count of clock ticks, as /proc/stat and /proc/self/stat count them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tick(pub u64);

/// One CPU's line of [`STAT`]: its idle and iowait ticks, and all its
/// ticks.
#[derive(Debug, Clone, Copy)]
struct Ticks {
    idle: Tick,
    total: Tick,
}

/// Each CPU's ticks in one read of [`STAT`].
fn ticks(stat: &str) -> Result<std::collections::BTreeMap<Cpu, Ticks>, String> {
    let mut cpus = std::collections::BTreeMap::new();
    for line in stat.lines() {
        let mut words = line.split_whitespace();
        let Some(n) = words.next().and_then(|w| w.strip_prefix("cpu")) else {
            continue;
        };
        if n.is_empty() {
            continue;
        }
        let cpu = Cpu(n.parse().map_err(|_| format!("`{line}` names no CPU"))?);
        let counts: Vec<u64> = words
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| format!("`{line}` is not a line of ticks"))?;
        let (Some(idle), Some(iowait)) = (counts.get(3), counts.get(4)) else {
            return Err(format!("`{line}` has no idle and iowait ticks"));
        };
        let ticks = Ticks {
            idle: Tick(idle + iowait),
            total: Tick(counts.iter().sum()),
        };
        cpus.insert(cpu, ticks);
    }
    Ok(cpus)
}

/// Each of `cores` whose share of ticks neither idle nor iowait, between
/// two reads of [`STAT`], is at least `max`. A core that did not tick is
/// idle. A core either read does not name is an error.
pub fn busy(before: &str, after: &str, cores: &CoreSet, max: BusyPct) -> Result<Vec<Busy>, String> {
    let (before, after) = (ticks(before)?, ticks(after)?);
    let mut found = Vec::new();
    for &cpu in &cores.0 {
        let (Some(a), Some(b)) = (before.get(&cpu), after.get(&cpu)) else {
            return Err(format!("no line for {cpu}"));
        };
        let total = b.total.0.saturating_sub(a.total.0);
        let idle = b.idle.0.saturating_sub(a.idle.0);
        let pct = if total > 0 {
            BusyPct(100.0 * total.saturating_sub(idle) as f64 / total as f64)
        } else {
            BusyPct(0.0)
        };
        if pct >= max {
            found.push(Busy { cpu, pct });
        }
    }
    Ok(found)
}

/// Whether a GPU is busy: both of its reads at least `max`.
pub fn gpu_busy(readings: [BusyPct; 2], max: BusyPct) -> bool {
    readings.iter().all(|r| *r >= max)
}

/// A percent, as `gpu_busy_percent` or a variable spells one.
pub fn percent(text: &str) -> Result<BusyPct, String> {
    let text = text.trim();
    match text.parse::<f64>() {
        Ok(p) if (0.0..=100.0).contains(&p) => Ok(BusyPct(p)),
        _ => Err(format!("`{text}` is not a percent")),
    }
}

/// A file's text, or an error naming it: absent and unreadable alike, for
/// a file that must be read.
fn text(read: Read, path: &Path) -> Result<String, String> {
    match read {
        Read::Text(text) => Ok(text),
        Read::Absent => Err(format!("{}: absent", path.display())),
        Read::Unreadable(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The lines a pinned measurement is held to.
struct Lines {
    busy: BusyPct,
    sample: Duration,
    gpu: BusyPct,
}

/// The lines from the environment, each defaulted when unset; the GPU's is
/// read only when the GPU is.
fn lines(m: &impl Machine, load: Load) -> Result<Lines, String> {
    let line = |name: &str, default: BusyPct| match m.var(name) {
        None => Ok(default),
        Some(v) => percent(&v).map_err(|e| format!("{name}: {e}")),
    };
    let sample = match m.var(QUIET_SAMPLE) {
        None => Duration::from_millis(500),
        Some(v) => Duration::from_millis(
            v.trim()
                .parse()
                .map_err(|_| format!("{QUIET_SAMPLE}: `{v}` is not a number of milliseconds"))?,
        ),
    };
    Ok(Lines {
        busy: line(QUIET_BUSY, BusyPct(5.0))?,
        sample,
        gpu: match load {
            Load::Cores => QUIET_GPU_BUSY_DEFAULT,
            Load::CoresAndGpu => line(QUIET_GPU_BUSY, QUIET_GPU_BUSY_DEFAULT)?,
        },
    })
}

/// A GPU's two reads of its busy percent, [`GPU_GAP`] apart.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// Its name in [`DRM`].
    pub name: String,
    /// What it read, in order.
    pub reads: [BusyPct; 2],
}

/// Every GPU with a `gpu_busy_percent`, read twice [`GPU_GAP`] apart. A card
/// with no such file is not read; one that cannot be read is an error
/// naming it.
fn gpu_pairs(m: &impl Machine) -> Result<Vec<Card>, String> {
    let names = match m.drm() {
        Read::Absent => return Ok(Vec::new()),
        Read::Unreadable(e) => return Err(format!("{DRM}: {e}")),
        Read::Text(names) => names,
    };
    let mut first = Vec::new();
    for name in names.lines() {
        let card = name
            .strip_prefix("card")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !card {
            continue;
        }
        let path = Path::new(DRM).join(name).join("device/gpu_busy_percent");
        let reading = match m.read(&path) {
            Read::Absent => continue,
            read => text(read, &path)?,
        };
        let reading = percent(&reading).map_err(|e| format!("{}: {e}", path.display()))?;
        first.push((name, path, reading));
    }
    if first.is_empty() {
        return Ok(Vec::new());
    }
    m.sleep(GPU_GAP);
    let mut cards = Vec::new();
    for (name, path, a) in first {
        let b = percent(&text(m.read(&path), &path)?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        cards.push(Card {
            name: name.to_string(),
            reads: [a, b],
        });
    }
    Ok(cards)
}

/// Each GPU busy on both of its two reads, as a line naming the card and
/// its readings.
fn gpu_load(m: &impl Machine, max: BusyPct) -> Result<Vec<String>, String> {
    Ok(gpu_pairs(m)?
        .into_iter()
        .filter(|c| gpu_busy(c.reads, max))
        .map(|c| {
            format!(
                "{} {} then {} busy, at or above {QUIET_GPU_BUSY}'s {max}",
                c.name, c.reads[0], c.reads[1]
            )
        })
        .collect())
}

/// Whether a measurement pinned to some CPUs may start: unpinned, nothing
/// is read past its affinity; pinned, its cores and their siblings must
/// each be under [`QUIET_BUSY`] over [`QUIET_SAMPLE`], and with
/// [`Load::CoresAndGpu`] every GPU under [`QUIET_GPU_BUSY`] on one of two
/// reads. What cannot be read refuses.
pub fn on_own_cores(m: &impl Machine, load: Load) -> Verdict {
    match judge(m, load) {
        Ok(verdict) => verdict,
        Err(e) => Verdict::Refused(format!(
            "whether this measurement's cores are quiet cannot be read ({e})"
        )),
    }
}

fn judge(m: &impl Machine, load: Load) -> Result<Verdict, String> {
    let status = text(m.read(Path::new(STATUS)), Path::new(STATUS))?;
    let allowed = allowed(&status).map_err(|e| format!("{STATUS}: {e}"))?;
    let online = text(m.read(Path::new(ONLINE)), Path::new(ONLINE))?;
    let online = cpu_list(&online).map_err(|e| format!("{ONLINE}: {e}"))?;
    let measures = measures(m)?;
    let pinned = match placement(&allowed, &online) {
        Placement::Unpinned => return Ok(Verdict::Unpinned),
        Placement::Pinned(_) if measures == Measures::Counts => return Ok(Verdict::Counting),
        Placement::Pinned(pinned) => pinned,
    };
    let quiet = lines(m, load)?;
    let cores = siblings(&pinned, |cpu| m.read(&siblings_path(cpu)))?;
    let before = text(m.read(Path::new(STAT)), Path::new(STAT))?;
    m.sleep(quiet.sample);
    let after = text(m.read(Path::new(STAT)), Path::new(STAT))?;
    let busy = busy(&before, &after, &cores, quiet.busy).map_err(|e| format!("{STAT}: {e}"))?;
    let gpus = match load {
        Load::Cores => Vec::new(),
        Load::CoresAndGpu => gpu_load(m, quiet.gpu)?,
    };
    if busy.is_empty() && gpus.is_empty() {
        return Ok(Verdict::Quiet);
    }
    let on = match load {
        Load::Cores => "",
        Load::CoresAndGpu => " or on the GPU",
    };
    let mut why = format!(
        "there is load on this measurement's cores ({cores}: its affinity and their SMT siblings){on}, \
         and a measurement must not share them:"
    );
    for b in &busy {
        why.push_str(&format!(
            "\n  {} {} busy, at or above {QUIET_BUSY}'s {}",
            b.cpu, b.pct, quiet.busy
        ));
    }
    for g in &gpus {
        why.push_str(&format!("\n  {g}"));
    }
    why.push_str("\nstart again once it has ended, or pin the measurement to quiet cores");
    Ok(Verdict::Refused(why))
}

/// This process's own statistics: its 14th and 15th fields are its user
/// and system time in ticks.
pub const SELF_STAT: &str = "/proc/self/stat";

/// This process's CPU time, user and system, from its [`SELF_STAT`] text.
/// The fields are counted after the command's closing parenthesis, since a
/// command may hold spaces and parentheses.
pub fn self_ticks(stat: &str) -> Result<Tick, String> {
    let (_, rest) = stat
        .rsplit_once(')')
        .ok_or_else(|| String::from("no command in parentheses"))?;
    // The state, the third field, is the first after the command.
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let field = |at: usize, name: &str| {
        fields
            .get(at)
            .and_then(|f| f.parse::<u64>().ok())
            .ok_or_else(|| format!("no {name} field"))
    };
    Ok(Tick(field(11, "utime")? + field(12, "stime")?))
}

/// The ticks a group of cores spent between two reads: busy, and in all.
struct Spent {
    busy: Tick,
    total: Tick,
}

fn spent(before: &str, after: &str, cores: &CoreSet) -> Result<Spent, String> {
    let (before, after) = (ticks(before)?, ticks(after)?);
    let (mut busy, mut total) = (0, 0);
    for &cpu in &cores.0 {
        let (Some(a), Some(b)) = (before.get(&cpu), after.get(&cpu)) else {
            return Err(format!("no line for {cpu}"));
        };
        let all = b.total.0.saturating_sub(a.total.0);
        total += all;
        busy += all.saturating_sub(b.idle.0.saturating_sub(a.idle.0));
    }
    Ok(Spent {
        busy: Tick(busy),
        total: Tick(total),
    })
}

/// `part` as a share of `whole`; nothing ticked is idle.
fn share(part: Tick, whole: Tick) -> BusyPct {
    if whole.0 > 0 {
        BusyPct(100.0 * part.0 as f64 / whole.0 as f64)
    } else {
        BusyPct(0.0)
    }
}

/// The share of a group of cores' ticks that were busy between two reads
/// of [`STAT`], or `None` for a group of no cores.
pub fn group_busy(before: &str, after: &str, cores: &CoreSet) -> Result<Option<BusyPct>, String> {
    if cores.0.is_empty() {
        return Ok(None);
    }
    let spent = spent(before, after, cores)?;
    Ok(Some(share(spent.busy, spent.total)))
}

/// Others' load on the measurement's own cores: their busy ticks between
/// two reads of [`STAT`], less this process's CPU time over the same
/// interval (`mine`, before and after), as a share of all their ticks. The
/// measurement keeps its own cores busy, so their busy alone cannot say
/// whether anything else ran there.
pub fn others_busy(
    before: &str,
    after: &str,
    own: &CoreSet,
    mine: [Tick; 2],
) -> Result<BusyPct, String> {
    let spent = spent(before, after, own)?;
    let mine = mine[1].0.saturating_sub(mine[0].0);
    Ok(share(Tick(spent.busy.0.saturating_sub(mine)), spent.total))
}

/// The online CPUs that are neither the measurement's own nor their
/// siblings.
pub fn rest_of(online: &CoreSet, own: &CoreSet, siblings: &CoreSet) -> CoreSet {
    online
        .0
        .iter()
        .filter(|c| !own.0.contains(c) && !siblings.0.contains(c))
        .copied()
        .collect()
}

/// What a run shared its machine with, over the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Shares {
    /// The measurement's own cores: the online CPUs of its affinity.
    pub own: CoreSet,
    /// Others' load on them, this process's own time taken out.
    pub others: BusyPct,
    /// Their SMT siblings outside the affinity, busy in full, since the
    /// measurement does not run there; `None` when there are none.
    pub siblings: Option<BusyPct>,
    /// Every other online CPU; `None` when there are none.
    pub rest: Option<BusyPct>,
    /// The busiest GPU, as the lower of its two reads; `None` for a
    /// measurement that does not draw, or a machine with no GPU file.
    pub gpu: Option<BusyPct>,
    /// The run's length.
    pub over: Duration,
}

impl std::fmt::Display for Shares {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let or_none =
            |p: Option<BusyPct>| p.map_or_else(|| String::from("none"), |p| p.to_string());
        write!(
            f,
            "own cores {} others {}; siblings {}; rest of the machine {} busy",
            self.own,
            self.others,
            or_none(self.siblings),
            or_none(self.rest)
        )?;
        if let Some(gpu) = self.gpu {
            write!(f, "; gpu {gpu}")?;
        }
        Ok(())
    }
}

/// Why a pinned run's conditions void its result, or `None`: others' load
/// on its own cores, or its siblings' load, at or above `busy`, or the GPU
/// at or above `gpu`. The rest of the machine is recorded and never voids.
pub fn void_reason(shares: &Shares, busy: BusyPct, gpu: BusyPct) -> Option<String> {
    let mut why = Vec::new();
    if shares.others >= busy {
        why.push(format!(
            "others' load on own cores {} was {}, at or above {QUIET_BUSY}'s {busy}",
            shares.own, shares.others
        ));
    }
    if let Some(siblings) = shares.siblings.filter(|s| *s >= busy) {
        why.push(format!(
            "their siblings were {siblings} busy, at or above {QUIET_BUSY}'s {busy}"
        ));
    }
    if let Some(g) = shares.gpu.filter(|g| *g >= gpu) {
        why.push(format!(
            "the gpu was {g} busy, at or above {QUIET_GPU_BUSY}'s {gpu}"
        ));
    }
    (!why.is_empty()).then(|| why.join("; "))
}

/// What a run's conditions were.
#[derive(Debug, Clone, PartialEq)]
pub enum Conditions {
    /// Read over the run.
    Measured(Shares),
    /// An unpinned run's, which could not be read, and why.
    Unread(String),
    /// None to read: this machine shows no per-CPU ticks.
    NotHere,
    /// A counting run's: no load was read.
    Counting,
}

/// A run whose result does not stand, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Void(pub String);

/// Whether a run's cores are its affinity alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pin {
    Pinned,
    Unpinned,
}

/// The CPUs a run's conditions are read over.
struct Groups {
    pin: Pin,
    own: CoreSet,
    siblings: CoreSet,
    rest: CoreSet,
}

/// The run's groups: unpinned, every online CPU is its own.
fn groups(m: &impl Machine) -> Result<Groups, String> {
    let status = text(m.read(Path::new(STATUS)), Path::new(STATUS))?;
    let allowed = allowed(&status).map_err(|e| format!("{STATUS}: {e}"))?;
    let online = text(m.read(Path::new(ONLINE)), Path::new(ONLINE))?;
    let online = cpu_list(&online).map_err(|e| format!("{ONLINE}: {e}"))?;
    match placement(&allowed, &online) {
        Placement::Unpinned => Ok(Groups {
            pin: Pin::Unpinned,
            own: online,
            siblings: CoreSet::default(),
            rest: CoreSet::default(),
        }),
        Placement::Pinned(own) => {
            let all = siblings(&own, |cpu| m.read(&siblings_path(cpu)))?;
            let siblings: CoreSet = all.0.difference(&own.0).copied().collect();
            let rest = rest_of(&online, &own, &siblings);
            Ok(Groups {
                pin: Pin::Pinned,
                own,
                siblings,
                rest,
            })
        }
    }
}

/// One read of the machine's ticks and this process's.
struct Snapshot {
    stat: String,
    mine: Tick,
}

fn snapshot(m: &impl Machine) -> Result<Snapshot, String> {
    let mine = text(m.read(Path::new(SELF_STAT)), Path::new(SELF_STAT))?;
    let mine = self_ticks(&mine).map_err(|e| format!("{SELF_STAT}: {e}"))?;
    let stat = text(m.read(Path::new(STAT)), Path::new(STAT))?;
    Ok(Snapshot { stat, mine })
}

/// A run's start as [`quiet_begin`] read it.
enum Start {
    NotHere,
    Counting,
    Unread(Pin, String),
    Read(Begun),
}

struct Begun {
    groups: Groups,
    lines: Option<Lines>,
    at: Snapshot,
}

/// A run in progress: what its start read, for [`Quiet::end`] to compare.
#[must_use = "a run's conditions are read only by `end`"]
pub struct Quiet {
    load: Load,
    began: Instant,
    start: Start,
}

/// Begin a run: read the ticks of the machine and of this process, and on
/// Linux nothing else is read until [`Quiet::end`]. Off Linux there is
/// nothing to read.
pub fn quiet_begin(load: Load) -> Quiet {
    if cfg!(target_os = "linux") {
        begin_on(&Host, load)
    } else {
        Quiet {
            load,
            began: Instant::now(),
            start: Start::NotHere,
        }
    }
}

/// [`quiet_begin`] on `m`.
pub fn begin_on(m: &impl Machine, load: Load) -> Quiet {
    let start = begun(m, load);
    Quiet {
        load,
        began: Instant::now(),
        start,
    }
}

fn begun(m: &impl Machine, load: Load) -> Start {
    match measures(m) {
        Ok(Measures::Counts) => return Start::Counting,
        Ok(Measures::Time) => {}
        Err(why) => return Start::Unread(Pin::Pinned, why),
    }
    // A run whose placement cannot be read is held as a pinned one.
    let groups = match groups(m) {
        Ok(groups) => groups,
        Err(why) => return Start::Unread(Pin::Pinned, why),
    };
    let lines = match groups.pin {
        Pin::Unpinned => None,
        Pin::Pinned => match lines(m, load) {
            Ok(lines) => Some(lines),
            Err(why) => return Start::Unread(Pin::Pinned, why),
        },
    };
    match snapshot(m) {
        Ok(at) => Start::Read(Begun { groups, lines, at }),
        Err(why) => Start::Unread(groups.pin, why),
    }
}

/// What cannot be read voids a pinned run and is recorded for an unpinned
/// one.
fn unread(pin: Pin, why: String) -> Result<Conditions, Void> {
    match pin {
        Pin::Pinned => Err(Void(format!("the run's conditions cannot be read ({why})"))),
        Pin::Unpinned => Ok(Conditions::Unread(why)),
    }
}

/// What the run shared its machine with, read at its end.
fn measure(m: &impl Machine, begun: &Begun, load: Load, began: Instant) -> Result<Shares, String> {
    let after = snapshot(m)?;
    let over = began.elapsed();
    let (before, groups) = (&begun.at, &begun.groups);
    let in_stat = |e: String| format!("{STAT}: {e}");
    let others = others_busy(
        &before.stat,
        &after.stat,
        &groups.own,
        [before.mine, after.mine],
    )
    .map_err(in_stat)?;
    let siblings = group_busy(&before.stat, &after.stat, &groups.siblings).map_err(in_stat)?;
    let rest = group_busy(&before.stat, &after.stat, &groups.rest).map_err(in_stat)?;
    let gpu = match load {
        Load::Cores => None,
        Load::CoresAndGpu => gpu_pairs(m)?
            .iter()
            .map(|c| {
                if c.reads[0] <= c.reads[1] {
                    c.reads[0]
                } else {
                    c.reads[1]
                }
            })
            .fold(None, |most: Option<BusyPct>, p| match most {
                Some(most) if most >= p => Some(most),
                _ => Some(p),
            }),
    };
    Ok(Shares {
        own: groups.own.clone(),
        others,
        siblings,
        rest,
        gpu,
        over,
    })
}

impl Quiet {
    /// End a run: read what it shared its machine with, and print it to
    /// stderr as one `quiet:` line, or a `void:` line naming why its result
    /// does not stand. A run that voids prints no rate and exits non-zero.
    /// An unpinned run, which the start's guard stands in for, never voids.
    pub fn end(self) -> Result<Conditions, Void> {
        let ended = self.end_on(&Host);
        match &ended {
            Ok(Conditions::Measured(shares)) => eprintln!("quiet: {shares}"),
            Ok(Conditions::Unread(why)) => eprintln!("quiet: conditions not read ({why})"),
            Ok(Conditions::NotHere) => {}
            Ok(Conditions::Counting) => eprintln!("quiet: counting; no load read"),
            Err(Void(why)) => eprintln!("void: {why}"),
        }
        ended
    }

    /// [`Quiet::end`] on `m`, printing nothing.
    pub fn end_on(self, m: &impl Machine) -> Result<Conditions, Void> {
        let begun = match self.start {
            Start::NotHere => return Ok(Conditions::NotHere),
            Start::Counting => return Ok(Conditions::Counting),
            Start::Unread(pin, why) => return unread(pin, why),
            Start::Read(begun) => begun,
        };
        let shares = match measure(m, &begun, self.load, self.began) {
            Ok(shares) => shares,
            Err(why) => return unread(begun.groups.pin, why),
        };
        if let Some(lines) = &begun.lines {
            if let Some(why) = void_reason(&shares, lines.busy, lines.gpu) {
                return Err(Void(why));
            }
        }
        Ok(Conditions::Measured(shares))
    }
}
