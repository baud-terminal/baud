//! The production path, end to end: launches a built baud the way the Dock
//! does -- with launchd's environment, which names no locale -- and checks
//! what its shell was given. No unit test can: every test process inherits
//! the runner's own `LANG`, so `Pty::fork`'s read of the user's locale never
//! runs there, and a `Pty::fork` that stopped asking would leave every test
//! green.
//!
//! `BAUD_CMD` writes the shell's `$LANG` and the number of characters
//! `wc -m` counts in one Hebrew letter (U+05D9, two bytes) to a file;
//! `BAUD_STARTUP` makes `shell_locale` print its `startup: locale:` note when
//! it is asked.
//!
//! Arms, each a fresh launch with only what launchd gives an app (`HOME`,
//! `USER`, `LOGNAME`, `TMPDIR`, `__CF_USER_TEXT_ENCODING`, a system `PATH`,
//! `SHELL`):
//!
//! - `dock`: no locale variable. `LANG` is a UTF-8 name, the letter is one
//!   character, and the note is printed (the read ran);
//! - `empty`: `LANG=` set empty. Replaced as in `dock`, never doubled;
//! - `control`: `LANG=fr_FR.UTF-8`. Kept as given, the letter one character,
//!   and no note (the read was skipped).
//!
//! Exits 1, printing what each arm saw, when any arm disagrees. It opens a
//! window, so nothing in a suite runs it: `dock-launch-check <binary>`.

use super::{Env, py};
use std::ffi::{CString, OsString};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What launchd passes on from the user's session.
const LAUNCHD: [&str; 5] = [
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "__CF_USER_TEXT_ENCODING",
];

/// `startup: locale: <name> in <ms> ms`: the name and the time's text.
pub fn note(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("startup: locale: ")?;
    let (name, rest) = rest.split_at(rest.find(py::is_space)?);
    let ms = rest.strip_prefix(" in ")?.strip_suffix(" ms")?;
    let plain =
        !name.is_empty() && !ms.is_empty() && ms.chars().all(|c| c.is_ascii_digit() || c == '.');
    plain.then_some((name, ms))
}

/// The environment an arm's launch has.
fn launchd_env(env: &Env, extra: Option<(&str, &str)>, out: &str) -> Vec<(OsString, OsString)> {
    let mut e: Vec<(OsString, OsString)> = LAUNCHD
        .iter()
        .filter_map(|k| env.iter().find(|(n, _)| n.to_str() == Some(k)).cloned())
        .collect();
    // Written whole and renamed, so a half-written file is never read.
    let cmd = format!(
        "printf '%s|' \"$LANG\" > '{out}.part'; \
         printf '\\327\\231' | /usr/bin/wc -m >> '{out}.part'; \
         mv '{out}.part' '{out}'; sleep 1"
    );
    for (k, v) in [
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
        ("SHELL", "/bin/zsh"),
        ("BAUD_STARTUP", "1"),
        ("BAUD_GRID", "80x24"),
        ("BAUD_CMD", cmd.as_str()),
    ] {
        e.push((k.into(), v.into()));
    }
    if let Some((k, v)) = extra {
        e.push((k.into(), v.into()));
    }
    e
}

/// What one launch left in the file, and the notes it printed.
type Seen = (Option<String>, Vec<(String, String)>);

fn launch(binary: &str, env: Vec<(OsString, OsString)>, out: &Path) -> Result<Seen, String> {
    let mut child = Command::new(binary)
        .env_clear()
        .envs(env)
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("{binary}: {e}"))?;
    let mut stderr = child.stderr.take().expect("stderr was piped");
    // A thread, because the pipe has to drain while the file is waited for.
    let reader = std::thread::spawn(move || {
        let mut all = Vec::new();
        let _ = stderr.read_to_end(&mut all);
        all
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    while !out.exists() && Instant::now() < deadline && matches!(child.try_wait(), Ok(None)) {
        std::thread::sleep(Duration::from_millis(50));
    }
    super::stop(&mut child, Duration::from_secs(10));
    let err = reader.join().unwrap_or_default();
    let err = String::from_utf8(err).map_err(|e| format!("stderr: {e}"))?;
    let seen = out.exists().then(|| fs::read_to_string(out)).transpose();
    let seen = seen.map_err(|e| format!("{}: {e}", out.display()))?;
    let notes = py::splitlines(&err.replace("\r\n", "\n").replace('\r', "\n"))
        .into_iter()
        .filter_map(note)
        .map(|(n, ms)| (n.to_string(), ms.to_string()))
        .collect();
    Ok((seen, notes))
}

/// `notes` as Python prints a list of pairs of strings.
fn repr_notes(notes: &[(String, String)]) -> String {
    let inner: Vec<String> = notes
        .iter()
        .map(|(a, b)| format!("({}, {})", py::repr_str(a), py::repr_str(b)))
        .collect();
    format!("[{}]", inner.join(", "))
}

/// Whether `path` is something this user can run.
fn runnable(path: &str) -> bool {
    CString::new(path).is_ok_and(|p| {
        // SAFETY: `p` is a live NUL-terminated string.
        unsafe { libc::access(p.as_ptr(), libc::X_OK) == 0 }
    })
}

/// Runs the program; the return value is its exit code.
pub fn run(args: &[String], env: &Env, out: &mut dyn Write) -> i32 {
    let Some(binary) = args.first() else {
        eprintln!("usage: dock-launch-check <binary>");
        return 1;
    };
    if !runnable(binary) {
        eprintln!("no binary at {binary}: build it with `cargo build --release -p baud`");
        return 1;
    }
    let scratch = match super::scratch_dir(env, "dock-launch-check-") {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut failures = Vec::new();
    for (arm, extra) in [
        ("dock", None),
        ("empty", Some(("LANG", ""))),
        ("control", Some(("LANG", "fr_FR.UTF-8"))),
    ] {
        let dir = scratch.join(arm);
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("{}: {e}", dir.display());
            return 1;
        }
        let file = dir.join("seen");
        let launched = launch(
            binary,
            launchd_env(env, extra, &file.display().to_string()),
            &file,
        );
        let _ = fs::remove_dir_all(&dir);
        let (seen, notes) = match launched {
            Ok(got) => got,
            Err(e) => {
                let _ = fs::remove_dir_all(&scratch);
                eprintln!("{e}");
                return 1;
            }
        };
        let seen = seen.unwrap_or_default();
        let (lang, count) = seen.split_once('|').unwrap_or((seen.as_str(), ""));
        let count = py::strip(count);
        let ok = if arm == "control" {
            lang == "fr_FR.UTF-8" && count == "1" && notes.is_empty()
        } else {
            lang.ends_with(".UTF-8") && count == "1" && notes.len() == 1 && notes[0].0 == lang
        };
        let _ = writeln!(
            out,
            "{} {arm:<8} LANG={} wc -m={} notes={}",
            if ok { "ok  " } else { "FAIL" },
            py::repr_str(lang),
            py::repr_str(count),
            repr_notes(&notes)
        );
        if !ok {
            failures.push(arm);
        }
    }
    let _ = fs::remove_dir_all(&scratch);
    if !failures.is_empty() {
        eprintln!("failed: {}", failures.join(", "));
        return 1;
    }
    0
}
