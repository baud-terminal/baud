//! **Shell integration: baud marks the prompts of the shells it starts.**
//! A shell that marks its prompt, where its input begins and where a
//! command's output begins and ends (`OSC 133`, `[[source:
//! semantic-prompts]]`) lets the terminal tell finished output from the
//! prompt the user is at: the word wrap redraws finished text at its words
//! (`baud_render::rebreak`), and the exit-status marks, the prompt jumps
//! and a completion list kept on screen all read the same marks. zsh and
//! bash send none of their own, and fish only from version 4 (measured:
//! zsh 5.9, bash 5.3, fish 3.7.1 none; fish 4.9.3 `A`, `B`, `C`, `D`).
//!
//! So, as kitty and Ghostty do (`[[source:
//! kitty-6d5d0c4-shell-integration-py]]`, `[[source:
//! ghostty-b58ef1d-shell-integration-zig]]`), baud starts each shell it
//! knows by a way in its own manual gives into its startup, and hands the
//! startup back to the user's own files at once: the scripts in `shell/`,
//! written for baud from the semantic prompts text and the shells'
//! manuals, embedded in the binary and written before the fork to a
//! directory only the user can write ([`scripts_dir`]).
//!
//! * **zsh**: `ZDOTDIR` names baud's directory, whose `.zshenv` puts the
//!   user's `ZDOTDIR` back (or unsets it), runs zsh's new-user check where
//!   zsh runs it and sources the user's `.zshenv`; zsh then reads
//!   `.zprofile`, `.zshrc` and `.zlogin` from the user's directory. zsh
//!   reads `$ZDOTDIR/.zshenv`, "If ZDOTDIR is unset, HOME is used instead"
//!   (`[[source: zsh-5-9-files]]`), and loads its newuser module just
//!   before it (`[[source: zsh-5-9-init-c]]`), which looks for the user's
//!   files in `ZDOTDIR` (`[[source: zsh-5-9-newuser-c]]`) -- baud's
//!   directory then, so the shim loads the module again. The marks go in
//!   the `precmd` and `preexec` hooks (`[[source: zsh-5-9-func]]`) and in
//!   the prompts inside `%{...%}` (`[[source: zsh-5-9-prompt]]`).
//! * **bash**: `--posix` and `ENV`, the one way into a login shell's
//!   startup: "When bash is started in posix mode, as with the --posix
//!   command line option ... interactive shells expand the ENV variable
//!   and read and execute commands from the file whose name is the
//!   expanded value. No other startup files are read", where `--rcfile`
//!   replaces `~/.bashrc` only for a shell that is not a login shell
//!   (`[[source: bash-5-3-man]]`, INVOCATION). The script puts `ENV`,
//!   POSIX mode and what it set back, and reads `/etc/profile` and the
//!   first of `~/.bash_profile`, `~/.bash_login` and `~/.profile`, as
//!   bash.1 has a login shell read them. Not macOS's `/bin/bash`: Apple's
//!   3.2 reads no `ENV` in POSIX mode (measured), as Ghostty's injection
//!   records (`[[source: ghostty-b58ef1d-shell-integration-zig]]`), and
//!   would start with none of the user's files. The marks go in
//!   `PROMPT_COMMAND` and in the prompts: from bash 5.1 one entry of
//!   baud's follows the user's, in an array whose first entry is the
//!   user's string as it was, so each entry runs with the command's `$?`,
//!   `PIPESTATUS` and `$_`; an exported string, and any before 5.1, gets
//!   baud's entries first and last in it. They run with stdout on
//!   `/dev/null`, which bash-preexec's DEBUG trap passes over, so it finds
//!   the user's first command after it installs itself at the first prompt.
//!   An exported prompt has its marks taken out before each line's first
//!   simple command by a DEBUG trap baud's last entry sets and its first
//!   firing clears, the user's own trap put back and run; a line that
//!   begins with `( ... )` or `coproc` runs that first with the marks in.
//! * **fish**: `-C`, which fish runs "after reading the configuration but
//!   before executing command specified by -c or reading interactive
//!   input" (`[[source: fish-cmd-fish]]`); nothing in its environment
//!   changes, where kitty's and Ghostty's way in, `XDG_DATA_DIRS`
//!   (`[[source: kitty-6d5d0c4-shell-integration-py]]`), changes which
//!   vendor directories fish 3 reads when it was unset (measured). The
//!   marks come from its `fish_prompt`, `fish_preexec` and `fish_postexec`
//!   events alone (`[[source: fish-language]]`), with no `B`: fish 3 draws
//!   its prompt from the user's own `fish_prompt`, and a function of baud's
//!   in its place is what `funcsave` would write over the user's file. An
//!   open departure from the text, by which a prompt "must end with
//!   'OSC 133;B' or 'OSC 133;I'". The
//!   script adds nothing to fish 4, which marks its own prompts
//!   (`[[source: fish-terminal-compat]]`).
//!
//! Only these three, and only a login shell from `$SHELL`: `BAUD_CMD`'s
//! command is started as it is. `[shell] integration = "off"` turns it
//! off. Each script sets `BAUD_SHELL_INTEGRATION` as a shell variable,
//! never exported, which the user's own zsh and bash startup files can
//! test (fish reads its configuration before baud's script runs), and
//! adds nothing where those files already mark the prompt.

/// `[shell] integration`: whether baud marks the prompts of the zsh, bash
/// and fish it starts. **On by default**, so a stock shell's latest output
/// is drawn at its words like the rest; `Off` starts every shell exactly as
/// it would start anywhere else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Integration {
    On,
    Off,
}

/// The variables baud's injection sets for a script to put back: none of
/// them reaches the user's own startup files, each script unsetting its
/// own first. They are dropped from the parent's environment at every
/// spawn (`pty::OUTER`), so a baud started inside a shell that somehow
/// kept one passes none of them on.
pub(crate) const HELPERS: [&str; 3] = [ZSH_ZDOTDIR, BASH_ENV, MARKER];

/// zsh's own `ZDOTDIR`, while baud's directory stands in it.
const ZSH_ZDOTDIR: &str = "BAUD_ZSH_ZDOTDIR";
/// bash's own `ENV`, while baud's script stands in it.
const BASH_ENV: &str = "BAUD_BASH_ENV";
/// The marker each script sets as a shell variable, never exported; listed
/// so an exported one from elsewhere is dropped rather than passed on.
const MARKER: &str = "BAUD_SHELL_INTEGRATION";

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) use inject::{Injection, inject};

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod inject {
    use super::{BASH_ENV, Integration, ZSH_ZDOTDIR};
    use std::ffi::{CString, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    /// zsh's startup shim, written as `zsh/.zshenv`.
    const ZSHENV: &str = include_str!("shell/zshenv.zsh");
    /// bash's, which `ENV` names.
    const BASH: &str = include_str!("shell/baud.bash");
    /// fish's, which `-C` sources.
    const FISH: &str = include_str!("shell/baud.fish");

    /// The three files and where each is written under the scripts'
    /// directory.
    const FILES: [(&str, &str); 3] = [
        ("zsh/.zshenv", ZSHENV),
        ("bash/baud.bash", BASH),
        ("fish/baud.fish", FISH),
    ];

    /// The shells baud has a script for.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum Kind {
        Zsh,
        Bash,
        Fish,
    }

    impl Kind {
        /// The shell `shell` names by its last component, a login shell's
        /// leading `-` dropped; `None` for any other.
        pub(super) fn of(shell: &str) -> Option<Kind> {
            let name = shell.rsplit('/').next().unwrap_or(shell);
            match name.strip_prefix('-').unwrap_or(name) {
                "zsh" => Some(Kind::Zsh),
                "bash" => Some(Kind::Bash),
                "fish" => Some(Kind::Fish),
                _ => None,
            }
        }
    }

    /// **What the spawn changes for a shell baud marks**: the arguments
    /// after argv[0], and the environment's entries set, each replacing
    /// one of its name.
    #[derive(Debug, PartialEq, Eq)]
    pub struct Injection {
        pub args: Vec<CString>,
        pub env: Vec<(OsString, OsString)>,
    }

    impl Injection {
        /// `env` with this injection's entries in place of any of their
        /// names.
        pub fn apply(&self, env: &mut Vec<CString>) {
            env.retain(|e| {
                !self.env.iter().any(|(k, _)| {
                    e.as_bytes()
                        .strip_prefix(k.as_bytes())
                        .is_some_and(|rest| rest.first() == Some(&b'='))
                })
            });
            env.extend(self.env.iter().filter_map(|(k, v)| {
                let mut entry = k.as_bytes().to_vec();
                entry.push(b'=');
                entry.extend_from_slice(v.as_bytes());
                CString::new(entry).ok()
            }));
        }
    }

    /// The value of `key` in `env`, the first as `getenv` reads it.
    fn var<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a OsString> {
        env.iter()
            .find(|(k, _)| k.as_bytes() == key.as_bytes())
            .map(|(_, v)| v)
    }

    /// **What baud adds to the start of `shell`**, found at `path`, under
    /// `integration`, with the parent's environment `parent`: `None` when
    /// it is off, when the shell is not one of the three, when it is
    /// macOS's `/bin/bash`, or when the scripts cannot be written where
    /// only the user can write them -- the shell then starts as it would
    /// without baud.
    pub fn inject(
        integration: Integration,
        shell: &str,
        path: &str,
        parent: &[(OsString, OsString)],
    ) -> Option<Injection> {
        if integration == Integration::Off {
            return None;
        }
        let kind = Kind::of(shell)?;
        if kind == Kind::Bash && apples_bash(Path::new(path)) {
            return None;
        }
        let dir = match scripts_dir(parent).and_then(|d| write_scripts(&d).map(|()| d)) {
            Ok(d) => d,
            Err(e) => {
                crate::warn!("shell integration: no marks for this shell: {e}");
                return None;
            }
        };
        Some(injection(kind, &dir, parent))
    }

    /// The arguments and variables for `kind`, its scripts under `dir`.
    pub(super) fn injection(kind: Kind, dir: &Path, parent: &[(OsString, OsString)]) -> Injection {
        let os = |s: &str| OsString::from(s);
        let at = |p: &str| dir.join(p).into_os_string();
        let kept = |own: &str, helper: &str| {
            var(parent, own)
                .map(|v| (os(helper), v.clone()))
                .into_iter()
                .collect::<Vec<_>>()
        };
        match kind {
            Kind::Zsh => {
                let mut env = kept("ZDOTDIR", ZSH_ZDOTDIR);
                env.push((os("ZDOTDIR"), at("zsh")));
                Injection {
                    args: Vec::new(),
                    env,
                }
            }
            Kind::Bash => {
                let mut env = kept("ENV", BASH_ENV);
                env.push((os("ENV"), at("bash/baud.bash")));
                Injection {
                    args: vec![c"--posix".to_owned()],
                    env,
                }
            }
            Kind::Fish => {
                let mut source = b"source '".to_vec();
                source.extend_from_slice(at("fish/baud.fish").as_bytes());
                source.push(b'\'');
                Injection {
                    args: [c"-C".to_owned()]
                        .into_iter()
                        .chain(CString::new(source).ok())
                        .collect(),
                    env: Vec::new(),
                }
            }
        }
    }

    /// **Whether `path` is macOS's own bash**: `/bin/bash`, once every
    /// link is followed. Apple's 3.2 reads no `ENV` in POSIX mode
    /// (measured), and `/bin` is the system's, read-only under System
    /// Integrity Protection, so nothing else can be there. Never on Linux.
    fn apples_bash(path: &Path) -> bool {
        cfg!(target_os = "macos")
            && std::fs::canonicalize(path).is_ok_and(|p| p == Path::new("/bin/bash"))
    }

    /// **The directory the scripts are written to, made and checked**:
    /// `$XDG_RUNTIME_DIR/baud`, else `$TMPDIR/baud-<uid>`, else
    /// `/tmp/baud-<uid>`, the first given as an absolute path. The XDG
    /// directory "MUST be owned by the user, and he MUST be the only one
    /// having read and write access to it" (`[[source: xdg-basedir-0-8]]`),
    /// and macOS's `TMPDIR` is the user's own; `/tmp` is everyone's, so the
    /// directory is made `0700` and, whoever made it, used only while it
    /// is a directory and not a link, the user's, and closed to group and
    /// others -- or another user could put a `.zshenv` in it. Within it,
    /// one directory per set of scripts, by their content's hash, so two
    /// versions of baud running at once never share one. A path a shell
    /// would read a character of as quoting or expansion (bash expands
    /// `ENV`) is refused.
    pub(super) fn scripts_dir(parent: &[(OsString, OsString)]) -> std::io::Result<PathBuf> {
        let absolute = |key: &str| {
            var(parent, key)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        // SAFETY: `getuid` has no preconditions and cannot fail.
        let uid = unsafe { crate::sys::getuid() };
        let own = format!("baud-{uid}");
        let base = absolute("XDG_RUNTIME_DIR")
            .map(|d| d.join("baud"))
            .or_else(|| absolute("TMPDIR").map(|d| d.join(&own)))
            .unwrap_or_else(|| Path::new("/tmp").join(&own));
        let dir = base.join(format!("shell-{}", digest()));
        let plain = dir
            .as_os_str()
            .as_bytes()
            .iter()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'$' | b'`' | b'\\' | b'\'' | b'"'));
        if !plain {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} has a character a shell would expand", dir.display()),
            ));
        }
        private_dir(&base, uid)?;
        Ok(dir)
    }

    /// `dir` made `0700` if it is not there, and then checked: a directory,
    /// not a link, owned by `uid`, with no permission for group or others.
    fn private_dir(dir: &Path, uid: crate::sys::Uid) -> std::io::Result<()> {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        match std::fs::DirBuilder::new()
            .recursive(false)
            .mode(0o700)
            .create(dir)
        {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // The base itself is missing: made as the user's alone.
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(dir)?;
            }
            Err(e) => return Err(e),
        }
        let m = std::fs::symlink_metadata(dir)?;
        let refuse = |why: &str| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{} {why}", dir.display()),
            ))
        };
        if !m.file_type().is_dir() {
            return refuse("is not a directory");
        }
        if m.uid() != uid {
            return refuse("belongs to another user");
        }
        if m.mode() & 0o077 != 0 {
            return refuse("is open to other users");
        }
        Ok(())
    }

    /// **The scripts written under `dir`**, each only when what is there
    /// differs, through a file of this process's own renamed over it, so a
    /// shell starting in another window never reads one half-written.
    pub(super) fn write_scripts(dir: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        for (name, text) in FILES {
            let file = dir.join(name);
            if std::fs::read(&file).is_ok_and(|had| had == text.as_bytes()) {
                continue;
            }
            if let Some(parent) = file.parent() {
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)?;
            }
            let mut part = file.clone().into_os_string();
            part.push(format!(".{}", std::process::id()));
            std::fs::write(&part, text)?;
            std::fs::rename(&part, &file)?;
        }
        Ok(())
    }

    /// FNV-1a over the three scripts, in order, in hex: a different
    /// script, a different directory.
    fn digest() -> String {
        let h = FILES
            .iter()
            .flat_map(|(name, text)| name.bytes().chain(text.bytes()))
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            });
        format!("{h:016x}")
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::inject::{Kind, injection, scripts_dir, write_scripts};
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("baud-shell-unit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch dir");
        d
    }

    /// The three shells by their last component, a login shell's `-`
    /// dropped; any other shell, and a name that only ends in one, none.
    #[test]
    fn the_three_shells_are_known_by_name() {
        for (shell, kind) in [
            ("/bin/zsh", Some(Kind::Zsh)),
            ("-zsh", Some(Kind::Zsh)),
            ("/usr/local/bin/bash", Some(Kind::Bash)),
            ("/opt/homebrew/bin/fish", Some(Kind::Fish)),
            ("/bin/sh", None),
            ("/usr/bin/nu", None),
            ("/usr/bin/zsh-5.9", None),
            ("/usr/bin/rbash", None),
        ] {
            assert_eq!(Kind::of(shell), kind, "{shell}");
        }
    }

    /// **zsh gets `ZDOTDIR`, the user's own kept beside it**: none when
    /// the parent had none, which the shim reads as unset.
    #[test]
    fn zsh_gets_zdotdir_and_keeps_the_users() {
        let dir = Path::new("/run/user/1000/baud/shell-x");
        let none = injection(Kind::Zsh, dir, &env(&[("HOME", "/h")]));
        assert_eq!(none.args, Vec::<std::ffi::CString>::new());
        assert_eq!(
            none.env,
            env(&[("ZDOTDIR", "/run/user/1000/baud/shell-x/zsh")])
        );
        let own = injection(Kind::Zsh, dir, &env(&[("ZDOTDIR", "/h/.config/zsh")]));
        assert_eq!(
            own.env,
            env(&[
                ("BAUD_ZSH_ZDOTDIR", "/h/.config/zsh"),
                ("ZDOTDIR", "/run/user/1000/baud/shell-x/zsh"),
            ])
        );
    }

    /// **bash gets `--posix` and `ENV`, the user's `ENV` kept beside it.**
    #[test]
    fn bash_gets_posix_and_env_and_keeps_the_users() {
        let dir = Path::new("/d");
        let i = injection(Kind::Bash, dir, &env(&[("ENV", "/h/.shrc")]));
        assert_eq!(i.args, [c"--posix".to_owned()]);
        assert_eq!(
            i.env,
            env(&[("BAUD_BASH_ENV", "/h/.shrc"), ("ENV", "/d/bash/baud.bash")])
        );
    }

    /// **fish gets `-C` and nothing in its environment.**
    #[test]
    fn fish_gets_an_init_command_and_no_variable() {
        let i = injection(
            Kind::Fish,
            Path::new("/d"),
            &env(&[("XDG_DATA_DIRS", "/x")]),
        );
        assert_eq!(
            i.args,
            [c"-C".to_owned(), c"source '/d/fish/baud.fish'".to_owned()]
        );
        assert!(i.env.is_empty());
    }

    /// **Off, an unknown shell and macOS's `/bin/bash` get nothing**, and
    /// nothing is written for them.
    #[test]
    fn off_an_unknown_shell_and_macos_bash_get_nothing() {
        let d = scratch("none");
        let parent = env(&[("TMPDIR", d.to_str().expect("UTF-8"))]);
        assert_eq!(
            inject(Integration::Off, "/bin/zsh", "/bin/zsh", &parent),
            None
        );
        assert_eq!(inject(Integration::On, "/bin/sh", "/bin/sh", &parent), None);
        if cfg!(target_os = "macos") {
            assert_eq!(
                inject(Integration::On, "/bin/bash", "/bin/bash", &parent),
                None
            );
        }
        assert!(
            std::fs::read_dir(&d).expect("the dir").next().is_none(),
            "nothing written"
        );
        // The control: zsh on gets its directory.
        assert!(inject(Integration::On, "/bin/zsh", "/bin/zsh", &parent).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **The scripts' directory is the user's alone**: made `0700` where
    /// it was not, and refused where something else stands at its name --
    /// a directory others may write, or a link -- so no other user's
    /// `.zshenv` is ever read. `XDG_RUNTIME_DIR` wins over `TMPDIR`.
    #[test]
    fn the_scripts_directory_is_the_users_alone() {
        let d = scratch("dir");
        let parent = env(&[("TMPDIR", d.to_str().expect("UTF-8"))]);
        // SAFETY: `getuid` has no preconditions.
        let own = d.join(format!("baud-{}", unsafe { crate::sys::getuid() }));
        let got = scripts_dir(&parent).expect("a fresh directory");
        assert!(got.starts_with(&own), "{got:?} under {own:?}");
        let mode = std::fs::metadata(&own).expect("made").permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "made 0700");
        // Open to others: refused.
        std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(
            scripts_dir(&parent).is_err(),
            "a directory others may read is refused"
        );
        std::fs::remove_dir_all(&own).expect("rm");
        // A link in its place: refused, whatever it points at.
        let target = d.join("elsewhere");
        std::fs::create_dir(&target).expect("a target");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        std::os::unix::fs::symlink(&target, &own).expect("a link");
        assert!(scripts_dir(&parent).is_err(), "a link is refused");
        // XDG_RUNTIME_DIR first.
        let xdg = d.join("run");
        std::fs::create_dir(&xdg).expect("a runtime dir");
        let both = env(&[
            ("TMPDIR", d.to_str().expect("UTF-8")),
            ("XDG_RUNTIME_DIR", xdg.to_str().expect("UTF-8")),
        ]);
        let got = scripts_dir(&both).expect("the runtime dir");
        assert!(got.starts_with(xdg.join("baud")), "{got:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **The scripts are written whole, and again only when they differ**:
    /// each file as embedded, and one changed on disk put back.
    #[test]
    fn the_scripts_are_written_whole() {
        let d = scratch("write");
        write_scripts(&d).expect("written");
        for (name, text) in [
            ("zsh/.zshenv", include_str!("shell/zshenv.zsh")),
            ("bash/baud.bash", include_str!("shell/baud.bash")),
            ("fish/baud.fish", include_str!("shell/baud.fish")),
        ] {
            assert_eq!(std::fs::read_to_string(d.join(name)).expect(name), text);
        }
        std::fs::write(d.join("zsh/.zshenv"), "echo tampered\n").expect("changed");
        write_scripts(&d).expect("written again");
        assert_eq!(
            std::fs::read_to_string(d.join("zsh/.zshenv")).expect("zshenv"),
            include_str!("shell/zshenv.zsh")
        );
        let left: Vec<_> = std::fs::read_dir(d.join("zsh"))
            .expect("zsh")
            .map(|e| e.expect("an entry").file_name())
            .collect();
        assert_eq!(left, [OsString::from(".zshenv")], "no part file left");
        let _ = std::fs::remove_dir_all(&d);
    }
}
