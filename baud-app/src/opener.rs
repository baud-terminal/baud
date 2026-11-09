//! **Opening a link on Linux**: `xdg-open`, which "opens a file or URL in
//! the user's preferred application" (`[[source: xdg-open-1]]`), for what
//! `link::admit` opens -- `http`, `https`, `mailto` -- and never for a
//! `file:` link, which `admit` marks to reveal, since opening a file can
//! launch it; that goes to the file manager over the session bus
//! (`bus.rs`). The URI is one argument with no shell, and `admit` has
//! already refused one beginning with `-`, which xdg-open "can not handle".
//! xdg-open may run "for a very long time" and may want a terminal, so it
//! is given none: its standard streams are the null device, and it is
//! reaped without waiting on each turn of the loop.
//!
//! **The reaper is both drivers'**: a `[link.*]` rule's command (`rules.rs`)
//! is started the same way, on macOS too, with the same null streams, and
//! reaped the same way on each tick.

use std::process::{Child, Command, Stdio};

/// The command that opens `uri`.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn command(uri: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

/// The openers started and not yet reaped.
#[derive(Default)]
pub(crate) struct Opener {
    children: Vec<Child>,
}

impl Opener {
    /// Start `cmd`, kept until it is reaped. Linux's, on its loop's next
    /// turn (`next_turn.rs`); macOS starts its commands on the effect
    /// queue's turn (`os_effect.rs`) and keeps them here.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) fn start(&mut self, mut cmd: Command) -> std::io::Result<()> {
        self.children.push(cmd.spawn()?);
        Ok(())
    }

    /// Keep `child`, started on a later turn, until it is reaped.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn keep(&mut self, child: Child) {
        self.children.push(child);
    }

    /// Reap the openers that have ended, never waiting.
    pub(crate) fn reap(&mut self) {
        self.children
            .retain_mut(|c| matches!(c.try_wait(), Ok(None)));
    }
}

/// This machine's name, for a `file://host/path` link -- the form `ls
/// --hyperlink` writes -- to be recognised as local: the kernel's, as
/// `gethostname` reads it. Empty when unreadable, which admits only the
/// empty and `localhost` hosts.
#[cfg(target_os = "linux")]
pub(crate) fn host_name() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The URI is one argument to `xdg-open`, with no shell between.**
    /// Perturbation: build `sh -c "xdg-open <uri>"` -- both assertions
    /// fail.
    #[test]
    fn a_link_is_one_argument_to_xdg_open() {
        let uri = "https://example.com/a b;$(x)";
        let c = command(uri);
        assert_eq!(c.get_program(), "xdg-open");
        let args: Vec<_> = c.get_args().collect();
        assert_eq!(args, [uri]);
    }

    /// **An opener that has ended is reaped, and one that runs is kept.**
    #[test]
    fn an_ended_opener_is_reaped_and_a_running_one_kept() {
        let mut o = Opener::default();
        let mut run = Command::new("/bin/sh");
        run.args(["-c", "read x"]).stdin(Stdio::piped());
        o.start(run).unwrap();
        o.reap();
        assert_eq!(o.children.len(), 1, "still running");
        let mut done = Command::new("/bin/sh");
        done.args(["-c", "exit 0"]);
        o.start(done).unwrap();
        // The ended one is reaped once the kernel has it: its wait is the
        // test's, so the loop's reap finds it ended.
        let mut ended = o.children.pop().unwrap();
        ended.wait().unwrap();
        o.children.push(ended);
        o.reap();
        assert_eq!(o.children.len(), 1, "the ended one gone");
        for c in &mut o.children {
            c.kill().unwrap();
            c.wait().unwrap();
        }
    }
}
