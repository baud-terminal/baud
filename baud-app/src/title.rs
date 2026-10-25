//! What a program's title becomes on the window.
//!
//! `OSC 0` and `OSC 2` set the title in the core (`Term::title`), and until
//! this module nothing read it: the window said "baud" whatever vim or ssh
//! asked for. The core keeps the bytes as they arrived, so turning them into
//! text a window can show is the embedder's, and it is pure so a test reaches
//! it -- the window it lands on is AppKit's or a compositor's.

use baud_core::PrintRun;
use baud_domain::InputEncoding;

/// The title to show for the bytes a program set, or `fallback` when it set
/// none or an empty one.
///
/// **The bytes are read in the encoding the core kept with them**
/// (`Term::title_encoding`). Under UTF-8 an ill-formed sequence becomes
/// U+FFFD, as the screen shows it; under ISO 8859-1 each byte is its own
/// codepoint, which is what that encoding means. A title set in one encoding
/// reads the same after a switch to the other.
///
/// **Controls become spaces**, C0, DEL and C1 alike. The parser drops most of
/// C0 from a string's body already, but a title is shown in a title bar and a
/// window list, where a stray control is not rendered the way a terminal
/// would, so this does not lean on that.
pub fn window_title(bytes: &[u8], encoding: InputEncoding, fallback: &str) -> String {
    let text: String = match encoding {
        InputEncoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        InputEncoding::Latin1 => bytes.iter().map(|&b| char::from(b)).collect(),
    };
    match shown(&text).as_str() {
        "" => fallback.to_string(),
        t => t.to_string(),
    }
}

/// Text as a title shows it: controls as spaces, the ends trimmed.
fn shown(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.trim().to_string()
}

/// Whether a title should be sent to the window: only when the bytes a
/// program set, their encoding, the printer's run or the derived fallback
/// changed since the last one sent. A read compares at most `OSC_MAX` bytes
/// and builds the derived title. A program that sets the same title on every
/// prompt costs the window nothing while the folder and program stay the
/// same. A change of the derived title sends the program's unchanged title
/// again, and on macOS sets the minimized window's title again.
pub struct Shown {
    last: Option<(Vec<u8>, InputEncoding, PrintRun, String)>,
    sent: Option<String>,
}

/// **The title while a printer controller run hides the output**: a program
/// sending its output to the printer leaves the screen as it was until it
/// is done (`[[source: dec-std-070]]` 070-7 § 7.3.6, "The video display is
/// not updated"), and the title says why, as the VT510 flashes "Printer" in
/// its indicator line when the printer stalls (`[[source: vt510-rm-b]]`
/// 10.6.3.2).
pub fn marked(title: String, run: PrintRun) -> String {
    match run {
        PrintRun::Controller => format!("{title} \u{2014} Printing"),
        PrintRun::Idle | PrintRun::AutoPrint => title,
    }
}

impl Shown {
    pub fn new() -> Shown {
        Shown {
            last: None,
            sent: None,
        }
    }

    /// The title last sent, which the minimized window's follows
    /// ([`Mini`]).
    #[cfg(any(target_os = "macos", test))]
    pub fn sent(&self) -> Option<&str> {
        self.sent.as_deref()
    }

    /// The title to send now, or `None` when the window already has it:
    /// a change of the bytes, their encoding, the printer's run or the
    /// fallback -- the folder and program a title is derived from -- is a
    /// change.
    pub fn next(
        &mut self,
        bytes: &[u8],
        encoding: InputEncoding,
        fallback: &str,
        run: PrintRun,
    ) -> Option<String> {
        if self.last.as_ref().is_some_and(|(b, e, r, f)| {
            b.as_slice() == bytes && *e == encoding && *r == run && f == fallback
        }) {
            return None;
        }
        self.last = Some((bytes.to_vec(), encoding, run, fallback.to_string()));
        let t = marked(window_title(bytes, encoding, fallback), run);
        self.sent = Some(t.clone());
        Some(t)
    }
}

/// **The icon name on macOS: the minimized window's title** (note AK
/// question 8, I1, decided by Jev at 0.92 both orders, decisions
/// 1209-1210). ctlseqs: "Ps = 1 -> Change Icon Name to Pt"; AppKit's
/// nearest place is `miniwindowTitle`, and "changing the full-size
/// `NSWindow` object's title ... automatically changes the minimized
/// window's title as well" (`[[source: apple-nswindow-miniwindowtitle]]`),
/// so it is set again after every title set. With no icon name the
/// minimized window shows the window's title, AppKit's own default. Read
/// in the encoding the core kept with it (`Term::icon_name_encoding`) and
/// shown as a title is. Wayland's `xdg_toplevel` has a title and no icon
/// name (`[[source: wp-xdg-shell]]`), so Linux keeps it stored.
#[cfg(any(target_os = "macos", test))]
pub struct Mini {
    sent: Option<String>,
}

#[cfg(any(target_os = "macos", test))]
impl Mini {
    pub fn new() -> Mini {
        Mini { sent: None }
    }

    /// The minimized window's title to set now, or `None` when it has it.
    /// `just_set` is the window's title if this turn set one, which AppKit
    /// set the minimized window's to; `title` is the window's title now.
    pub fn next(
        &mut self,
        just_set: Option<&str>,
        icon: &[u8],
        encoding: InputEncoding,
        title: &str,
    ) -> Option<String> {
        if let Some(t) = just_set {
            self.sent = Some(t.to_string());
        }
        let want = window_title(icon, encoding, title);
        if self.sent.as_deref() == Some(want.as_str()) {
            return None;
        }
        self.sent = Some(want.clone());
        Some(want)
    }
}

// **When a program sets no title, the folder and the program**: Jev's
// lean T2, 0.61/0.72 (note AK question 2, decisions 1207-1208): a
// program's title wins outright, as ctlseqs' "Change Window Title to Pt"
// has it, and the derived title stands only in place of a missing one, so
// nothing is added to what a program set. Spelled `folder — program`
// (FOLDER_PROGRAM, decided on a second ask, 0.95/0.98, decisions
// 1215-1216), the folder as [`folder_text`] spells it.

/// A process id, `pid_t`, as `sys.rs` declares it; spelled here so this
/// module stays pure and its tests run on every platform.
pub type Pid = core::ffi::c_int;

/// What a derived title is read from, so a test can hand it a fake
/// (`fg::Live` is the kernel's).
pub trait Reads {
    fn group(&self) -> Option<Pid>;
    fn name(&self, pid: Pid) -> Option<String>;
    fn dir(&self, pid: Pid) -> Option<std::path::PathBuf>;
}

/// Whether a check is owed: a read, a key or a process event since the
/// last one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owed {
    No,
    Yes,
}

/// Whether a check found anything new.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Changed {
    No,
    Yes,
}

/// Whether the program's name must be read again although its group did
/// not change: an exec renames a process in place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stale {
    No,
    Yes,
}

/// A count of the checks made, as a reading to compare.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Checks(pub u64);

/// A count of the `OSC 7` reports refused for naming another host, as a
/// reading to compare.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Refused(pub u64);

/// The last `OSC 7` report a title judged: its host (none for no host or
/// `localhost`) and its path, so a report read again is not judged again.
struct Judged {
    host: Option<Vec<u8>>,
    path: Vec<u8>,
}

impl Judged {
    fn is(&self, d: baud_core::WorkingDirectory<'_>) -> bool {
        self.path.as_slice() == d.path
            && match (&self.host, d.host) {
                (None, baud_core::FileHost::Local) => true,
                (Some(a), baud_core::FileHost::Named(b)) => a.as_slice() == b,
                _ => false,
            }
    }
}

/// **The folder and program a title is derived from**, and when they are
/// read again. **A check is owed, never timed**: a read, a key and, on
/// macOS, a process fork, exec or exit (`fg::Watch`) each owe one, and the
/// app makes it once, on its next display-link tick (macOS) or frame
/// (Linux), however many were owed -- a flood of reads costs one check a
/// frame, and an idle terminal none (note AK 6.5). A check reads the
/// group, 2.5 us measured on this Mac, and the directory, 0.45 us; the
/// name only for a new group or after a process event, argv[0] costing
/// 3.8-5.9 us (note AK 2.6).
pub struct Derive {
    owed: Owed,
    stale: Stale,
    group: Option<Pid>,
    name: Option<String>,
    dir: Option<std::path::PathBuf>,
    pub checks: Checks,
    judged: Option<Judged>,
    /// The reports refused for naming another host, each counted once
    /// however often a title is built while it stands ([`Derive::title`]).
    pub refused: Refused,
}

impl Derive {
    /// Owed from the start, so the first frame reads the shell.
    pub fn new() -> Derive {
        Derive {
            owed: Owed::Yes,
            stale: Stale::Yes,
            group: None,
            name: None,
            dir: None,
            checks: Checks(0),
            judged: None,
            refused: Refused(0),
        }
    }

    /// A read or a key: the foreground may have moved.
    pub fn owe(&mut self) {
        self.owed = Owed::Yes;
    }

    /// A process event: owed, and the name read again.
    #[cfg(any(target_os = "macos", test))]
    pub fn moved(&mut self) {
        self.owed = Owed::Yes;
        self.stale = Stale::Yes;
    }

    #[cfg(any(target_os = "macos", test))]
    pub fn owed(&self) -> Owed {
        self.owed
    }

    /// Whether a process event came since the last check.
    #[cfg(any(target_os = "macos", test))]
    pub fn moved_since_check(&self) -> bool {
        self.stale == Stale::Yes
    }

    /// The foreground group the last check read.
    #[cfg(any(target_os = "macos", test))]
    pub fn group(&self) -> Option<Pid> {
        self.group
    }

    /// The check, if one is owed.
    pub fn check(&mut self, r: &impl Reads) -> Changed {
        if self.owed == Owed::No {
            return Changed::No;
        }
        self.owed = Owed::No;
        self.checks = Checks(self.checks.0 + 1);
        let group = r.group();
        let name = match group != self.group || self.stale == Stale::Yes {
            true => group.and_then(|p| r.name(p)),
            false => self.name.clone(),
        };
        self.stale = Stale::No;
        let dir = group.and_then(|p| r.dir(p));
        let changed = match group == self.group && name == self.name && dir == self.dir {
            true => Changed::No,
            false => Changed::Yes,
        };
        (self.group, self.name, self.dir) = (group, name, dir);
        changed
    }

    /// **The derived title**: the folder, an em dash and the program, or
    /// whichever of the two is known. The folder is the one the shell's
    /// `OSC 7` named when it names this machine -- no host, `localhost`, or
    /// one of `hosts`, this machine's names (`[[source: rfc8089]]` § 2 and
    /// § 3, compared without case) -- and the program's own otherwise.
    /// **An `OSC 7` naming another host is refused** for the title and
    /// counted in [`Derive::refused`] (Jev's lean R1, 0.76/0.61, "Refuse a
    /// remote host for every use and count it"): "Treating a non-local file
    /// URI as local ... can result in security problems" (§ 5). A title is
    /// built on every read, so a report is counted when it is judged first,
    /// not on every build: the same report read again counts nothing, and
    /// a different one, or the same one after another, counts again.
    pub fn title(
        &mut self,
        cwd: Option<baud_core::WorkingDirectory<'_>>,
        hosts: &[String],
        home: Option<&[u8]>,
    ) -> Option<String> {
        let reported = cwd.and_then(|d| local(d, hosts));
        match cwd {
            Some(d) if self.judged.as_ref().is_some_and(|j| j.is(d)) => {}
            Some(d) => {
                self.judged = Some(Judged {
                    host: match d.host {
                        baud_core::FileHost::Local => None,
                        baud_core::FileHost::Named(h) => Some(h.to_vec()),
                    },
                    path: d.path.to_vec(),
                });
                if reported.is_none() {
                    self.refused = Refused(self.refused.0 + 1);
                }
            }
            None => self.judged = None,
        }
        let folder = match (reported, &self.dir) {
            (Some(p), _) => Some(folder_text(p, home)),
            (None, Some(d)) => Some(folder_text(
                std::os::unix::ffi::OsStrExt::as_bytes(d.as_os_str()),
                home,
            )),
            (None, None) => None,
        };
        let program = self.name.as_deref().map(shown);
        match (
            folder.filter(|f| !f.is_empty()),
            program.filter(|p| !p.is_empty()),
        ) {
            (Some(f), Some(p)) => Some(format!("{f} \u{2014} {p}")),
            (Some(t), None) | (None, Some(t)) => Some(t),
            (None, None) => None,
        }
    }

    /// The line the app logs when [`Derive::refused`] has grown: the host
    /// the last report named, shown as a title shows text, and the count.
    pub fn refusal(&self) -> String {
        let host = self
            .judged
            .as_ref()
            .and_then(|j| j.host.as_deref())
            .map(|h| shown(&String::from_utf8_lossy(h)))
            .unwrap_or_default();
        format!(
            "a shell's OSC 7 named the host \"{host}\", not this machine: the \
             title shows the program's own folder ({} refused)",
            self.refused.0
        )
    }
}

/// The path of an `OSC 7` report that names this machine, else `None`:
/// no host, `localhost`, or one of `hosts` without case. An empty name
/// matches nothing.
pub fn local<'d>(d: baud_core::WorkingDirectory<'d>, hosts: &[String]) -> Option<&'d [u8]> {
    match d.host {
        baud_core::FileHost::Local => Some(d.path),
        baud_core::FileHost::Named(h)
            if hosts
                .iter()
                .any(|n| !n.is_empty() && h.eq_ignore_ascii_case(n.as_bytes())) =>
        {
            Some(d.path)
        }
        baud_core::FileHost::Named(_) => None,
    }
}

/// **A folder as the title spells it**: the path with the home directory
/// as `~`, and past three folders an ellipsis and the last three --
/// `~/src/baud`, `…/baud/baud-core/src` -- as kitty's and ghostty's own
/// shell integrations title a prompt, with zsh's `%(4~|…/%3~|%~)` (kitty's
/// zsh integration 271-281, ghostty's 243-248). Counted as zsh counts,
/// measured: `~` is a folder and the root is not, so `/usr/share/man` is
/// whole and `/usr/share/man/man1` is `…/share/man/man1`. Jev's lean,
/// F-SHORT, on a second ask after reading those (0.25/0.37, decisions
/// 1215-1216), over the whole path and the last folder alone. Shown as a
/// title shows text.
pub fn folder_text(path: &[u8], home: Option<&[u8]>) -> String {
    let home = home.filter(|h| h.len() > 1 && !h.ends_with(b"/"));
    let spelled: Vec<u8> = match home {
        Some(h) if path == h => b"~".to_vec(),
        Some(h) if path.starts_with(h) && path.get(h.len()) == Some(&b'/') => {
            [&b"~"[..], &path[h.len()..]].concat()
        }
        _ => path.to_vec(),
    };
    let folders: Vec<&[u8]> = spelled
        .split(|&b| b == b'/')
        .filter(|f| !f.is_empty())
        .collect();
    let shortened = match folders.len() {
        0..=3 => spelled,
        n => ["\u{2026}/".as_bytes(), &folders[n - 3..].join(&b'/')].concat(),
    };
    shown(&String::from_utf8_lossy(&shortened))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kernel as a test writes it, counting what each check asked.
    struct Fake {
        group: Option<Pid>,
        name: &'static str,
        dir: &'static str,
        names: std::cell::Cell<u32>,
    }

    impl Fake {
        fn new(group: Pid, name: &'static str, dir: &'static str) -> Fake {
            Fake {
                group: Some(group),
                name,
                dir,
                names: std::cell::Cell::new(0),
            }
        }
    }

    impl Reads for Fake {
        fn group(&self) -> Option<Pid> {
            self.group
        }
        fn name(&self, _: Pid) -> Option<String> {
            self.names.set(self.names.get() + 1);
            Some(self.name.to_string())
        }
        fn dir(&self, _: Pid) -> Option<std::path::PathBuf> {
            Some(self.dir.into())
        }
    }

    const HOME: Option<&[u8]> = Some(b"/Users/me");

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    /// **A flood of owed checks is one check, and an unowed tick is none**:
    /// what keeps the reads off the per-read path.
    #[test]
    fn many_owed_checks_are_one_and_an_unowed_one_is_none() {
        let mut d = Derive::new();
        let k = Fake::new(7, "zsh", "/Users/me");
        assert_eq!(d.check(&k), Changed::Yes, "owed from the start");
        for _ in 0..100 {
            d.owe();
        }
        assert_eq!(d.owed(), Owed::Yes);
        assert_eq!(d.check(&k), Changed::No, "the same reading");
        assert_eq!(d.check(&k), Changed::No);
        assert_eq!(d.checks, Checks(2), "two checks for 101 owed");
        assert_eq!(d.group(), Some(7));
    }

    /// **The name is read for a new group or after a process event**, not
    /// on every check: argv[0] is the costly read (note AK 2.6).
    #[test]
    fn the_name_is_read_once_per_group_and_after_a_process_event() {
        let mut d = Derive::new();
        let shell = Fake::new(7, "zsh", "/Users/me");
        d.check(&shell);
        d.owe();
        d.check(&shell);
        assert_eq!(shell.names.get(), 1, "one group, one read");
        d.moved();
        assert!(d.moved_since_check());
        d.check(&shell);
        assert_eq!(shell.names.get(), 2, "an exec may have renamed it");
        assert!(!d.moved_since_check());
        let job = Fake::new(9, "vim", "/Users/me/src/baud");
        d.owe();
        assert_eq!(d.check(&job), Changed::Yes);
        assert_eq!(job.names.get(), 1, "a new group is read");
        assert_eq!(
            d.title(None, &names(&["mac"]), HOME).as_deref(),
            Some("~/src/baud \u{2014} vim")
        );
    }

    /// **The folder is the shell's `OSC 7` when it names this machine**,
    /// the program's own directory otherwise; **one naming another host is
    /// refused** (RFC 8089 § 5) and the program's directory stands.
    #[test]
    fn a_local_osc_7_names_the_folder_and_a_remote_one_does_not() {
        use baud_core::{FileHost, WorkingDirectory};
        let mut d = Derive::new();
        d.check(&Fake::new(7, "ssh", "/Users/me/src"));
        let at = |host, path| WorkingDirectory { host, path };
        let local = at(FileHost::Named(b"MAC.local"), b"/Users/me/My Code");
        assert_eq!(
            d.title(Some(local), &names(&["mac.local"]), HOME)
                .as_deref(),
            Some("~/My Code \u{2014} ssh"),
            "this machine, without case"
        );
        let bare = at(FileHost::Local, b"/tmp");
        assert_eq!(
            d.title(Some(bare), &names(&["mac.local"]), HOME).as_deref(),
            Some("/tmp \u{2014} ssh")
        );
        let remote = at(FileHost::Named(b"build-box"), b"/home/me/src");
        assert_eq!(
            d.title(Some(remote), &names(&["mac.local"]), HOME)
                .as_deref(),
            Some("~/src \u{2014} ssh"),
            "refused: the program's own folder"
        );
        let unknown = at(FileHost::Named(b"build-box"), b"/x");
        assert_eq!(
            d.title(Some(unknown), &names(&[""]), HOME).as_deref(),
            Some("~/src \u{2014} ssh"),
            "a machine with no name knows no named host"
        );
        let either = names(&["mac.local", "mac"]);
        let short = at(FileHost::Named(b"mac"), b"/tmp");
        assert_eq!(
            d.title(Some(short), &either, HOME).as_deref(),
            Some("/tmp \u{2014} ssh"),
            "either of this machine's names"
        );
    }

    /// **A report naming another host is counted once**, however often a
    /// title is built while it stands; a local one is not counted, and a
    /// different remote one, or the same one after another, counts again.
    #[test]
    fn a_remote_osc_7_is_counted_once_a_report() {
        use baud_core::{FileHost, WorkingDirectory};
        let (mut d, mac) = (Derive::new(), names(&["mac"]));
        let remote = WorkingDirectory {
            host: FileHost::Named(b"build-box"),
            path: b"/home/me/src",
        };
        d.title(Some(remote), &mac, HOME);
        d.title(Some(remote), &mac, HOME);
        assert_eq!(d.refused, Refused(1), "one report, built twice");
        assert_eq!(
            d.refusal(),
            "a shell's OSC 7 named the host \"build-box\", not this machine: \
             the title shows the program's own folder (1 refused)"
        );
        let mut l = Derive::new();
        for host in [FileHost::Local, FileHost::Named(b"MAC")] {
            l.title(
                Some(WorkingDirectory {
                    host,
                    path: b"/tmp",
                }),
                &mac,
                HOME,
            );
        }
        assert_eq!(l.refused, Refused(0), "a local report is not counted");
        let other = WorkingDirectory {
            host: FileHost::Named(b"build-box"),
            path: b"/home/me",
        };
        d.title(Some(other), &mac, HOME);
        assert_eq!(d.refused, Refused(2), "a different remote report");
        let here = WorkingDirectory {
            host: FileHost::Local,
            path: b"/tmp",
        };
        d.title(Some(here), &mac, HOME);
        d.title(Some(other), &mac, HOME);
        assert_eq!(d.refused, Refused(3), "the same one, after another");
    }

    /// The home directory is `~` only as a whole component, and a folder
    /// is shown as a title is: controls as spaces.
    #[test]
    fn a_folder_is_spelled_from_home_and_shown_as_a_title() {
        assert_eq!(folder_text(b"/Users/me", HOME), "~");
        assert_eq!(folder_text(b"/Users/me/a", HOME), "~/a");
        assert_eq!(folder_text(b"/Users/menot", HOME), "/Users/menot");
        assert_eq!(folder_text(b"/a\x1bb\x07", None), "/a b");
        assert_eq!(folder_text(b"/Users/me/x", Some(b"/")), "/Users/me/x");
    }

    /// **Past three folders, an ellipsis and the last three**, counted as
    /// zsh's `%(4~|…/%3~|%~)` counts them, each case measured against zsh:
    /// `~` is a folder, the root is not.
    #[test]
    fn past_three_folders_the_last_three_after_an_ellipsis() {
        for (path, want) in [
            (&b"/Users/me/src/baud"[..], "~/src/baud"),
            (
                b"/Users/me/src/baud/baud-core",
                "\u{2026}/src/baud/baud-core",
            ),
            (
                b"/Users/me/src/baud/baud-core/src",
                "\u{2026}/baud/baud-core/src",
            ),
            (b"/", "/"),
            (b"/usr/share/man", "/usr/share/man"),
            (b"/usr/share/man/man1", "\u{2026}/share/man/man1"),
        ] {
            assert_eq!(folder_text(path, HOME), want, "{path:?}");
        }
    }

    /// **A program's title wins outright**, and the derived title stands
    /// only in place of a missing one; a change of the derived title is a
    /// change of what is sent.
    #[test]
    fn a_programs_title_wins_and_the_derived_one_fills_in() {
        let mut s = Shown::new();
        let u = InputEncoding::Utf8;
        let derived = "~/src \u{2014} zsh";
        assert_eq!(
            s.next(b"", u, derived, PrintRun::Idle).as_deref(),
            Some(derived)
        );
        assert_eq!(s.next(b"", u, derived, PrintRun::Idle), None);
        assert_eq!(
            s.next(b"", u, "~/src \u{2014} vim", PrintRun::Idle)
                .as_deref(),
            Some("~/src \u{2014} vim"),
            "a new derived title is sent"
        );
        assert_eq!(
            s.next(b"main.rs - VIM", u, derived, PrintRun::Idle)
                .as_deref(),
            Some("main.rs - VIM"),
            "the program's own"
        );
        assert_eq!(s.next(b"main.rs - VIM", u, derived, PrintRun::Idle), None);
        assert_eq!(s.sent(), Some("main.rs - VIM"), "what the window has");
    }

    /// With nothing read, the derived title is whichever half is known.
    #[test]
    fn a_half_known_derived_title_is_that_half() {
        let mut d = Derive::new();
        assert_eq!(
            d.title(None, &names(&["mac"]), HOME),
            None,
            "nothing checked"
        );
        let blind = Fake {
            group: None,
            name: "x",
            dir: "/y",
            names: std::cell::Cell::new(0),
        };
        d.check(&blind);
        assert_eq!(
            d.title(None, &names(&["mac"]), HOME),
            None,
            "no group, nothing read"
        );
        let reported = baud_core::WorkingDirectory {
            host: baud_core::FileHost::Local,
            path: b"/Users/me/src",
        };
        assert_eq!(
            d.title(Some(reported), &names(&["mac"]), HOME).as_deref(),
            Some("~/src")
        );
    }

    /// The body of `fn <sig>` in `src`, up to its closing brace at the
    /// indent it opened at.
    fn body<'s>(src: &'s str, sig: &str, indent: &str) -> &'s str {
        assert_eq!(src.matches(sig).count(), 1, "one {sig}");
        let rest = src.split(sig).nth(1).expect("the fn");
        let end = format!("\n{indent}}}\n");
        &rest[..rest.find(&end).expect("the fn ends")]
    }

    /// **Both drivers owe the check where the foreground can move and make
    /// it once a frame, and send every title through one place.** The
    /// drivers are AppKit- and Wayland-bound and no test drives them, so
    /// this holds their source, as `app.rs` holds its pump's: a read and a
    /// key owe a check, macOS's process events too, the check runs on the
    /// tick (macOS) or where a frame is presented (Linux), and the only
    /// `titles.next` in each is `send_title`'s, which passes the derived
    /// title. Spelled apart, so this test's own text is not what it finds.
    #[test]
    fn both_drivers_owe_the_check_and_send_the_derived_title() {
        const MAC: &str = include_str!("app.rs");
        const LINUX: &str = include_str!("linux.rs");
        let owe = concat!("self.derive", ".owe();");
        let next = concat!("self.titles", ".next(");
        let pump = body(
            MAC,
            concat!("    fn pump(&mut self, ", "trigger: Trigger) {"),
            "    ",
        );
        assert!(pump.contains(owe), "a read owes it");
        assert!(
            pump.contains(concat!("self.derive", ".moved();")),
            "a process event"
        );
        let tick = pump
            .split(concat!("if let Trigger::Tick = ", "trigger {"))
            .nth(1)
            .expect("the tick");
        assert!(
            tick[..tick.find("\n        }\n").expect("the tick ends")]
                .contains(concat!("self.title", "_check();")),
            "checked on the tick"
        );
        assert!(
            body(
                MAC,
                concat!("    fn typed(&mut self, ", "e: KeyEvent) {"),
                "    "
            )
            .contains(owe),
            "a key"
        );
        let send = body(MAC, concat!("    fn send_title(", "&mut self) {"), "    ");
        assert!(send.contains(next) && send.contains("&fallback"));
        assert!(
            MAC.contains(concat!("hosts: machine", "_names(),"))
                && body(MAC, concat!("fn machine", "_names() -> Vec<String> {"), "")
                    .contains(concat!("hostlog::machine", "_name()")),
            "macOS compares with the name shells send"
        );
        assert_eq!(
            MAC.matches(next).count(),
            1,
            "macOS sends through send_title alone"
        );
        let feed = body(
            LINUX,
            concat!("    fn feed(&mut self, ", "bytes: &[u8]) {"),
            "    ",
        );
        assert!(feed.contains(owe), "a Linux read owes it");
        assert!(
            LINUX.contains(concat!("d.derive", ".owe();")),
            "a Linux key"
        );
        let present = body(LINUX, concat!("    fn present(", "&mut self) {"), "    ");
        assert!(
            present.contains(concat!("self.derive", ".check(")),
            "checked at a frame"
        );
        let send = body(LINUX, concat!("    fn send_title(", "&mut self) {"), "    ");
        assert!(send.contains(next) && send.contains("&fallback"));
        assert_eq!(
            LINUX.matches(next).count(),
            1,
            "Linux sends through send_title alone"
        );
    }

    /// **The icon name is the minimized window's title, set again after
    /// every title set**, since AppKit's title set overwrites it; with no
    /// icon name the minimized window keeps the window's title.
    #[test]
    fn the_icon_name_is_the_minimized_title_after_every_title_set() {
        let (mut m, u) = (Mini::new(), InputEncoding::Utf8);
        let vim = "main.rs - VIM";
        assert_eq!(
            m.next(Some(vim), b"", u, vim),
            None,
            "no icon name: the title's"
        );
        assert_eq!(m.next(None, b"main.rs", u, vim).as_deref(), Some("main.rs"));
        assert_eq!(m.next(None, b"main.rs", u, vim), None, "once");
        assert_eq!(
            m.next(Some("other - VIM"), b"main.rs", u, "other - VIM")
                .as_deref(),
            Some("main.rs"),
            "set again after the title set"
        );
        assert_eq!(
            m.next(None, b"", u, "other - VIM").as_deref(),
            Some("other - VIM"),
            "an emptied icon name gives the title back"
        );
        assert_eq!(
            m.next(None, b"caf\xe9\x07", InputEncoding::Latin1, "x")
                .as_deref(),
            Some("café"),
            "read in its encoding and shown as a title"
        );
    }

    /// **macOS sets the minimized window's title after the window's, in
    /// the one place titles are sent.** AppKit-bound, so the source is held.
    #[test]
    fn macos_sets_the_minimized_title_after_the_title() {
        const MAC: &str = include_str!("app.rs");
        let send = body(MAC, concat!("    fn send_title(", "&mut self) {"), "    ");
        let set = send
            .find(concat!("self.window", ".set_title("))
            .expect("the title set");
        let next = send.find(concat!("self.mini", ".next(")).expect("told");
        assert!(
            send[next..]
                .trim_start_matches(concat!("self.mini", ".next("))
                .trim_start()
                .starts_with("sent.as_deref(),"),
            "told of the title set"
        );
        let mini = send
            .find(concat!("setMiniwindow", "Title("))
            .expect("the minimized title");
        assert!(
            set < next && next < mini,
            "set, told, then the minimized title"
        );
        assert!(send.contains(concat!("icon_name", "_encoding()")));
        assert_eq!(MAC.matches(concat!("setMiniwindow", "Title(")).count(), 1);
    }

    #[test]
    fn a_utf8_title_is_its_text() {
        let t = window_title("vim — main.rs".as_bytes(), InputEncoding::Utf8, "baud");
        assert_eq!(t, "vim — main.rs");
    }

    #[test]
    fn an_empty_or_blank_title_is_the_fallback() {
        assert_eq!(window_title(b"", InputEncoding::Utf8, "baud"), "baud");
        assert_eq!(window_title(b"   ", InputEncoding::Utf8, "baud"), "baud");
    }

    #[test]
    fn controls_become_spaces_and_the_ends_are_trimmed() {
        assert_eq!(
            window_title(b"\ta\x7fb\x1bc ", InputEncoding::Utf8, "baud"),
            "a b c"
        );
        // C1 as a UTF-8 codepoint, U+0085.
        assert_eq!(
            window_title("x\u{85}y".as_bytes(), InputEncoding::Utf8, "baud"),
            "x y"
        );
    }

    #[test]
    fn ill_formed_utf8_is_the_replacement_character() {
        assert_eq!(
            window_title(b"a\xffb", InputEncoding::Utf8, "baud"),
            "a\u{fffd}b"
        );
    }

    #[test]
    fn latin1_bytes_are_their_own_codepoints() {
        assert_eq!(
            window_title(b"caf\xe9", InputEncoding::Latin1, "baud"),
            "café"
        );
        // And the same bytes under UTF-8 are not.
        assert_eq!(
            window_title(b"caf\xe9", InputEncoding::Utf8, "baud"),
            "caf\u{fffd}"
        );
    }

    #[test]
    fn a_title_is_sent_only_when_it_changes() {
        let mut s = Shown::new();
        let u = InputEncoding::Utf8;
        assert_eq!(
            s.next(b"", u, "baud", PrintRun::Idle),
            Some("baud".into()),
            "the first is sent"
        );
        assert_eq!(s.next(b"", u, "baud", PrintRun::Idle), None);
        assert_eq!(s.next(b"a", u, "baud", PrintRun::Idle), Some("a".into()));
        assert_eq!(s.next(b"a", u, "baud", PrintRun::Idle), None);
        assert_eq!(
            s.next(b"a", InputEncoding::Latin1, "baud", PrintRun::Idle),
            Some("a".into()),
            "a change of encoding is a change"
        );
        assert_eq!(
            s.next(b"", u, "baud", PrintRun::Idle),
            Some("baud".into()),
            "and back to none"
        );
        assert_eq!(
            s.next(b"", u, "baud", PrintRun::Controller),
            Some("baud \u{2014} Printing".into()),
            "a controller run marks the title"
        );
        assert_eq!(s.next(b"", u, "baud", PrintRun::Controller), None);
        assert_eq!(
            s.next(b"", u, "baud", PrintRun::AutoPrint),
            Some("baud".into()),
            "autoprint leaves the screen drawing, and no mark"
        );
    }
}
