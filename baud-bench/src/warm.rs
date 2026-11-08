//! **A measurement does not share the machine with a warm build.**
//! `scripts/warm.sh` keeps the release test build warm while you work:
//! `devenv up` starts a watcher, and the watcher a build at each save.
//! Every bench and timing example calls [`refuse_while_warm`] before it
//! measures anything, and refuses while a watcher or a build of one is
//! running and that checkout has not been paused (`warm-pause`), and on
//! Linux beside any process of a build, paused or not: one a pause has not
//! stopped is still on the CPU.
//!
//! An idle watcher counts, since the next save starts a build halfway
//! through the measurement. It is found on the whole machine, not in one
//! checkout: a timing A/B runs binaries built in a scratch copy of the tree,
//! which has no `.devenv/` of its own. The watcher and each build carry the
//! words `<root>/scripts/warm.sh build` in their command lines, and `<root>`
//! is where the pause is read.
//!
//! `bench-others/multibench` is a workspace of its own and links nothing
//! from this one, so it takes this file by `#[path]`.

use std::path::{Path, PathBuf};

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

/// Why a measurement must not start now, or `None`. A machine whose
/// processes cannot be listed is a reason: what cannot be seen is not known
/// to be quiet.
pub fn refusal() -> Option<String> {
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
