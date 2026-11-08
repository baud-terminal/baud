//! **A measurement does not share the machine with a warm build.**
//! `scripts/warm.sh` keeps the release test build warm while you work:
//! `devenv up` starts a watcher, and the watcher a build at each save.
//! Every bench and timing example calls [`refuse_while_warm`] before it
//! measures anything, and refuses while a watcher or a build of one is
//! running and that checkout has not been paused (`warm-pause`).
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

/// A watcher or a build, and the checkout its command line names.
#[derive(Debug, PartialEq, Eq)]
pub struct Warm {
    /// The process id, as the listing spells it.
    pub pid: String,
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
                _ => Some(Warm {
                    pid: pid.to_owned(),
                    root,
                }),
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
    if warm.is_empty() {
        return None;
    }
    let mut why = String::from(
        "a warm build or its watcher is running, and a measurement must not share the CPU with it:",
    );
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
