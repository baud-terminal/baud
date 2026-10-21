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

use std::process::{Child, Command, Stdio};

/// The command that opens `uri`.
pub(crate) fn command(uri: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

/// **A local `file:` URI with no authority**, the form a file manager is
/// handed: `link::admit` has already found the host empty, `localhost` or
/// this machine's, and RFC 8089 notes that "Some current usages of the
/// scheme incorrectly interpret all values in the authority of a file
/// URI, including "localhost", as non-local", so "To maximize
/// compatibility" the authority may be left out (`[[source: rfc8089]]`
/// § 2). `ls
/// --hyperlink` writes the host.
pub(crate) fn local(uri: &str) -> String {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return uri.to_owned();
    };
    match rest.find('/') {
        Some(at) => format!("{scheme}://{}", &rest[at..]),
        None => uri.to_owned(),
    }
}

/// The openers started and not yet reaped.
#[derive(Default)]
pub(crate) struct Opener {
    children: Vec<Child>,
}

impl Opener {
    /// Start `cmd`, kept until it is reaped.
    pub(crate) fn start(&mut self, mut cmd: Command) -> std::io::Result<()> {
        self.children.push(cmd.spawn()?);
        Ok(())
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

    /// **A reveal names the file with no host**: this machine's name and
    /// `localhost` are left out, and a URI with none is as it was.
    /// Perturbation: keep the authority -- the first assertion fails.
    #[test]
    fn a_revealed_file_names_no_host() {
        assert_eq!(
            local("file://thishost.local/tmp/a%20b"),
            "file:///tmp/a%20b"
        );
        assert_eq!(local("file://localhost/p"), "file:///p");
        assert_eq!(local("file:///p"), "file:///p");
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
