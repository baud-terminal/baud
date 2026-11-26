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
//! (default 500), or when a GPU's `gpu_busy_percent` reads at least
//! `BAUD_QUIET_GPU_BUSY` (default 10) on both of two reads 100 ms apart. A
//! watcher or a build on other CPUs is not load on its cores, so neither is
//! asked about. A GPU with no such file is not checked, and one whose file
//! cannot be read refuses; so does a CPU file absent or unreadable.
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
use std::time::Duration;

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
    if cfg!(target_os = "linux") {
        match on_own_cores(&Host) {
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
    if let Some(why) = refusal() {
        eprintln!("refused: {why}");
        std::process::exit(1);
    }
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
    if cfg!(target_os = "linux") && on_own_cores(&Host) == Verdict::Counting {
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

/// A count of a CPU's ticks.
#[derive(Debug, Clone, Copy)]
struct Tick(u64);

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
struct Quiet {
    busy: BusyPct,
    sample: Duration,
    gpu: BusyPct,
}

/// The lines from the environment, each defaulted when unset.
fn quiet(m: &impl Machine) -> Result<Quiet, String> {
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
    Ok(Quiet {
        busy: line(QUIET_BUSY, BusyPct(5.0))?,
        sample,
        gpu: line(QUIET_GPU_BUSY, BusyPct(10.0))?,
    })
}

/// Each GPU busy on both of two reads [`GPU_GAP`] apart, as a line naming
/// the card and its readings. A card with no `gpu_busy_percent` is not
/// checked; one that cannot be read is an error naming it.
fn gpu_load(m: &impl Machine, max: BusyPct) -> Result<Vec<String>, String> {
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
    let mut found = Vec::new();
    for (name, path, a) in first {
        let b = percent(&text(m.read(&path), &path)?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if gpu_busy([a, b], max) {
            found.push(format!(
                "{name} {a} then {b} busy, at or above {QUIET_GPU_BUSY}'s {max}"
            ));
        }
    }
    Ok(found)
}

/// Whether a measurement pinned to some CPUs may start: unpinned, nothing
/// is read past its affinity; pinned, its cores and their siblings must
/// each be under [`QUIET_BUSY`] over [`QUIET_SAMPLE`], and every GPU under
/// [`QUIET_GPU_BUSY`] on one of two reads. What cannot be read refuses.
pub fn on_own_cores(m: &impl Machine) -> Verdict {
    match judge(m) {
        Ok(verdict) => verdict,
        Err(e) => Verdict::Refused(format!(
            "whether this measurement's cores are quiet cannot be read ({e})"
        )),
    }
}

fn judge(m: &impl Machine) -> Result<Verdict, String> {
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
    let quiet = quiet(m)?;
    let cores = siblings(&pinned, |cpu| m.read(&siblings_path(cpu)))?;
    let before = text(m.read(Path::new(STAT)), Path::new(STAT))?;
    m.sleep(quiet.sample);
    let after = text(m.read(Path::new(STAT)), Path::new(STAT))?;
    let busy = busy(&before, &after, &cores, quiet.busy).map_err(|e| format!("{STAT}: {e}"))?;
    let gpus = gpu_load(m, quiet.gpu)?;
    if busy.is_empty() && gpus.is_empty() {
        return Ok(Verdict::Quiet);
    }
    let mut why = format!(
        "there is load on this measurement's cores ({cores}: its affinity and their SMT siblings) \
         or on the GPU, and a measurement must not share them:"
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
