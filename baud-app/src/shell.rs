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

/// `[shell] ssh_env`: whether the integration's zsh, bash and fish wrap
/// `ssh` so it asks to send `COLORTERM` and `TERM_PROGRAM`, `-o
/// SendEnv=COLORTERM -o SendEnv=TERM_PROGRAM` (ssh_config(5), `SendEnv`).
/// A server keeps one only where its sshd_config(5) `AcceptEnv` names it.
/// **Off by default**: it puts a function of baud's in front of your own
/// `ssh` command, which only you can ask for; an `ssh` function or alias of
/// yours is kept. Nothing with the integration off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SshEnv {
    Off,
    On,
}

/// The variables baud's injection sets for a script to put back: none of
/// them reaches the user's own startup files, each script unsetting its
/// own first. They are dropped from the parent's environment at every
/// spawn (`pty::OUTER`), so a baud started inside a shell that somehow
/// kept one passes none of them on.
pub(crate) const HELPERS: [&str; 5] = [ZSH_ZDOTDIR, BASH_ENV, MARKER, SSH_ENV, TOKEN];

/// zsh's own `ZDOTDIR`, while baud's directory stands in it.
const ZSH_ZDOTDIR: &str = "BAUD_ZSH_ZDOTDIR";
/// bash's own `ENV`, while baud's script stands in it.
const BASH_ENV: &str = "BAUD_BASH_ENV";
/// The marker each script sets as a shell variable, never exported; listed
/// so an exported one from elsewhere is dropped rather than passed on.
const MARKER: &str = "BAUD_SHELL_INTEGRATION";
/// `[shell] ssh_env = "on"`, for zsh's and bash's script, which each
/// unset it first and wrap `ssh` at its end.
const SSH_ENV: &str = "BAUD_SSH_ENV";
/// The token baud made for the window's shell (`ran::ShellToken`), for
/// zsh's and bash's script, which each take it into a shell variable and
/// unset it before the user's files run; fish's `-C` sets its variable.
const TOKEN: &str = "BAUD_SHELL_TOKEN";

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) use inject::{Injection, inject};

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod inject {
    use super::{BASH_ENV, Integration, SSH_ENV, SshEnv, TOKEN, ZSH_ZDOTDIR};
    use crate::child_env::{ChildEnv, EnvName};
    use crate::ran::ShellToken;
    use std::ffi::{CString, OsString};

    /// The names an injection sets, each checked when the build is.
    const ZDOTDIR: EnvName = EnvName::fixed("ZDOTDIR");
    const ENV: EnvName = EnvName::fixed("ENV");
    const ZSH_ZDOTDIR_NAME: EnvName = EnvName::fixed(ZSH_ZDOTDIR);
    const BASH_ENV_NAME: EnvName = EnvName::fixed(BASH_ENV);
    const SSH_ENV_NAME: EnvName = EnvName::fixed(SSH_ENV);
    const TOKEN_NAME: EnvName = EnvName::fixed(TOKEN);
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
        pub env: Vec<(EnvName, OsString)>,
        /// The token handed to the shell, which the window's marks are read
        /// against (`ran::Commands::new`).
        pub token: Option<ShellToken>,
    }

    impl Injection {
        /// `env` with this injection's entries in place of any of their
        /// names.
        pub fn apply(&self, env: &mut ChildEnv) {
            for (k, v) in &self.env {
                env.set(k, v.clone());
            }
        }
    }

    /// The value of `key` in `env`, the first as `getenv` reads it.
    fn var<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a OsString> {
        env.iter()
            .find(|(k, _)| k.as_bytes() == key.as_bytes())
            .map(|(_, v)| v)
    }

    /// **What baud adds to the start of `shell`**, found at `path`, under
    /// `integration` and `ssh_env`, with the parent's environment `parent`:
    /// `None` when
    /// it is off, when the shell is not one of the three, when it is
    /// macOS's `/bin/bash`, or when the scripts cannot be written where
    /// only the user can write them -- the shell then starts as it would
    /// without baud.
    pub fn inject(
        integration: Integration,
        ssh_env: SshEnv,
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
        let token = ShellToken::mint();
        Some(injection(kind, &dir, parent, ssh_env, token.as_ref()))
    }

    /// The arguments and variables for `kind`, its scripts under `dir`;
    /// with `ssh_env` on, zsh and bash told by [`SSH_ENV`] and fish by a
    /// shell variable its `-C` sets before the script; and `token`, the
    /// window's shell's, told the same ways ([`TOKEN`]), so fish's never
    /// reaches its environment and zsh's and bash's leaves it before the
    /// user's files run.
    pub(super) fn injection(
        kind: Kind,
        dir: &Path,
        parent: &[(OsString, OsString)],
        ssh_env: SshEnv,
        token: Option<&ShellToken>,
    ) -> Injection {
        let os = |s: &str| OsString::from(s);
        let at = |p: &str| dir.join(p).into_os_string();
        let kept = |own: &str, helper: &EnvName| {
            var(parent, own)
                .map(|v| (helper.clone(), v.clone()))
                .into_iter()
                .collect::<Vec<_>>()
        };
        let wrap = match ssh_env {
            SshEnv::On => Some((SSH_ENV_NAME, os("1"))),
            SshEnv::Off => None,
        };
        let told = token.map(|t| (TOKEN_NAME, os(t.as_str())));
        let kept_token = token.cloned();
        match kind {
            Kind::Zsh => {
                let mut env = kept("ZDOTDIR", &ZSH_ZDOTDIR_NAME);
                env.push((ZDOTDIR, at("zsh")));
                env.extend(wrap);
                env.extend(told);
                Injection {
                    args: Vec::new(),
                    env,
                    token: kept_token,
                }
            }
            Kind::Bash => {
                let mut env = kept("ENV", &BASH_ENV_NAME);
                env.push((ENV, at("bash/baud.bash")));
                env.extend(wrap);
                env.extend(told);
                Injection {
                    args: vec![c"--posix".to_owned()],
                    env,
                    token: kept_token,
                }
            }
            Kind::Fish => {
                let mut source = match ssh_env {
                    SshEnv::On => b"set -g __baud_ssh_env 1; ".to_vec(),
                    SshEnv::Off => Vec::new(),
                };
                // Hex digits alone: nothing fish reads as quoting.
                if let Some(t) = token {
                    source.extend_from_slice(b"set -g __baud_token ");
                    source.extend_from_slice(t.as_str().as_bytes());
                    source.extend_from_slice(b"; ");
                }
                source.extend_from_slice(b"source '");
                source.extend_from_slice(at("fish/baud.fish").as_bytes());
                source.push(b'\'');
                Injection {
                    args: [c"-C".to_owned()]
                        .into_iter()
                        .chain(CString::new(source).ok())
                        .collect(),
                    env: Vec::new(),
                    token: kept_token,
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

    /// An injection's entries as name and value, to compare with [`env`].
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn pairs(set: &[(crate::child_env::EnvName, OsString)]) -> Vec<(OsString, OsString)> {
        set.iter()
            .map(|(k, v)| (k.as_os_str().to_os_string(), v.clone()))
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
        let none = injection(Kind::Zsh, dir, &env(&[("HOME", "/h")]), SshEnv::Off, None);
        assert_eq!(none.args, Vec::<std::ffi::CString>::new());
        assert_eq!(
            pairs(&none.env),
            env(&[("ZDOTDIR", "/run/user/1000/baud/shell-x/zsh")])
        );
        let own = injection(
            Kind::Zsh,
            dir,
            &env(&[("ZDOTDIR", "/h/.config/zsh")]),
            SshEnv::Off,
            None,
        );
        assert_eq!(
            pairs(&own.env),
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
        let i = injection(
            Kind::Bash,
            dir,
            &env(&[("ENV", "/h/.shrc")]),
            SshEnv::Off,
            None,
        );
        assert_eq!(i.args, [c"--posix".to_owned()]);
        assert_eq!(
            pairs(&i.env),
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
            SshEnv::Off,
            None,
        );
        assert_eq!(
            i.args,
            [c"-C".to_owned(), c"source '/d/fish/baud.fish'".to_owned()]
        );
        assert!(i.env.is_empty());
    }

    /// **`[shell] ssh_env = "on"` tells each script, and off tells none**:
    /// zsh and bash `BAUD_SSH_ENV=1` beside what they get anyway, which each
    /// script unsets before the user's files run, and fish a shell variable
    /// its `-C` sets before the script, with nothing in its environment.
    /// The variable is one of the helpers a spawn drops from the parent's
    /// environment. Perturbation: tell the scripts whatever `ssh_env` says
    /// -- the off arms carry it.
    #[test]
    fn ssh_env_on_tells_each_script_and_off_tells_none() {
        let dir = Path::new("/d");
        let parent = env(&[("HOME", "/h")]);
        for (kind, base) in [
            (Kind::Zsh, env(&[("ZDOTDIR", "/d/zsh")])),
            (Kind::Bash, env(&[("ENV", "/d/bash/baud.bash")])),
        ] {
            let off = injection(kind, dir, &parent, SshEnv::Off, None);
            assert_eq!(pairs(&off.env), base, "{kind:?} off");
            let on = injection(kind, dir, &parent, SshEnv::On, None);
            let mut want = base.clone();
            want.extend(env(&[("BAUD_SSH_ENV", "1")]));
            assert_eq!(pairs(&on.env), want, "{kind:?} on");
            assert_eq!(on.args, off.args, "{kind:?}: the arguments as they were");
        }
        let fish = |s| injection(Kind::Fish, dir, &parent, s, None);
        assert_eq!(
            fish(SshEnv::Off).args,
            [c"-C".to_owned(), c"source '/d/fish/baud.fish'".to_owned()]
        );
        assert_eq!(
            fish(SshEnv::On).args,
            [
                c"-C".to_owned(),
                c"set -g __baud_ssh_env 1; source '/d/fish/baud.fish'".to_owned()
            ]
        );
        assert!(
            fish(SshEnv::On).env.is_empty(),
            "nothing in fish's environment"
        );
        assert!(
            HELPERS.contains(&"BAUD_SSH_ENV"),
            "dropped from the parent's"
        );
    }

    /// **B2: the token baud made is handed to the shell, one way a shell**:
    /// zsh and bash `BAUD_SHELL_TOKEN` beside what they get anyway, which
    /// each script takes into a shell variable and unsets before the user's
    /// files run, and fish a shell variable its `-C` sets before the script,
    /// with nothing in its environment; the injection keeps the token for
    /// the window's scan of the marks. With none made, none is told. The
    /// variable is one of the helpers a spawn drops from the parent's
    /// environment. Named break: tell no shell the token -- zsh's and
    /// bash's environment lack it.
    #[test]
    fn the_token_is_handed_to_each_shell() {
        let dir = Path::new("/d");
        let parent = env(&[("HOME", "/h")]);
        let token = crate::ran::ShellToken::given("0123456789abcdef");
        for kind in [Kind::Zsh, Kind::Bash] {
            let told = injection(kind, dir, &parent, SshEnv::Off, Some(&token));
            let mut want = pairs(&injection(kind, dir, &parent, SshEnv::Off, None).env);
            want.extend(env(&[("BAUD_SHELL_TOKEN", "0123456789abcdef")]));
            assert_eq!(pairs(&told.env), want, "{kind:?}");
            assert_eq!(told.token.as_ref(), Some(&token), "{kind:?}: kept");
        }
        let fish = injection(Kind::Fish, dir, &parent, SshEnv::Off, Some(&token));
        assert_eq!(
            fish.args,
            [
                c"-C".to_owned(),
                c"set -g __baud_token 0123456789abcdef; source '/d/fish/baud.fish'".to_owned()
            ]
        );
        assert!(fish.env.is_empty(), "nothing in fish's environment");
        let none = injection(Kind::Zsh, dir, &parent, SshEnv::Off, None);
        assert!(none.token.is_none(), "no token, none kept");
        assert!(
            !pairs(&none.env)
                .iter()
                .any(|(k, _)| k == "BAUD_SHELL_TOKEN"),
            "and none told"
        );
        assert!(
            HELPERS.contains(&"BAUD_SHELL_TOKEN"),
            "dropped from the parent's"
        );
        let minted = crate::ran::ShellToken::mint().expect("the random source");
        assert!(
            minted.as_str().len() == 16 && minted.as_str().bytes().all(|b| b.is_ascii_hexdigit()),
            "{minted:?}"
        );
    }

    /// **zsh and bash take the token out of the environment before any
    /// file of the user's runs, and no script makes a token of its own**:
    /// in each, the token is read and then unset above the first line that
    /// sources a file (`.` or `source` as a word), and no script reads the system's
    /// random source. A pure check of the sources, which holds on a machine
    /// with neither shell; the real shells run it in `pty/shell_tests.rs`.
    /// Named break: unset the token after the user's files -- the unset is
    /// below the first `.`.
    #[test]
    fn the_token_leaves_the_environment_before_the_users_files() {
        let zsh = include_str!("shell/zshenv.zsh");
        let bash = include_str!("shell/baud.bash");
        let fish = include_str!("shell/baud.fish");
        // Where the first line that sources a file begins: one, not a
        // comment, with `.` or `source` as a word of it.
        let first_source = |text: &str| {
            let mut at = 0;
            for line in text.split_inclusive('\n') {
                let code = !line.trim_start().starts_with('#');
                if code && line.split_whitespace().any(|w| w == "." || w == "source") {
                    return at;
                }
                at += line.len();
            }
            panic!("no file sourced")
        };
        for (name, text, read, unset) in [
            (
                "zsh",
                zsh,
                "__baud_token=${BAUD_SHELL_TOKEN-}",
                "builtin unset BAUD_SHELL_TOKEN",
            ),
            (
                "bash",
                bash,
                "__baud_token=${BAUD_SHELL_TOKEN-}",
                "unset BAUD_SHELL_TOKEN",
            ),
        ] {
            let (r, u) = (text.find(read).expect(read), text.find(unset).expect(unset));
            assert!(r < u, "{name}: read, then unset");
            assert!(
                u < first_source(text),
                "{name}: unset before the first file sourced"
            );
        }
        for (name, text) in [("zsh", zsh), ("bash", bash), ("fish", fish)] {
            assert!(!text.contains("urandom"), "{name}: no token of its own");
        }
    }

    /// **`inject` hands every shell it marks a token**: zsh, bash and fish
    /// each get one minted from the system's random source, sixteen hex
    /// digits, a different one each time. The shells are named by paths
    /// that cannot exist, so no machine's own shell, as Apple's bash is
    /// told apart by where its path leads, decides the test. Named break:
    /// mint none -- the injection keeps no token.
    #[test]
    fn inject_hands_each_shell_a_token() {
        let d = scratch("token");
        let parent = env(&[("TMPDIR", d.to_str().expect("UTF-8"))]);
        let absent = d.join("absent");
        let mut seen = Vec::new();
        for name in ["zsh", "bash", "fish"] {
            let path = absent.join(name);
            let shell = path.to_str().expect("UTF-8");
            let got = inject(Integration::On, SshEnv::Off, shell, shell, &parent)
                .unwrap_or_else(|| panic!("{shell}: injected"));
            let token = got.token.unwrap_or_else(|| panic!("{shell}: a token"));
            assert!(
                token.as_str().len() == 16 && token.as_str().bytes().all(|b| b.is_ascii_hexdigit()),
                "{shell}: {token:?}"
            );
            seen.push(token);
        }
        seen.dedup();
        assert_eq!(seen.len(), 3, "a token each");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **Each script wraps `ssh` one way, only when told, and keeps the
    /// user's own**: the one command it runs, `command ssh -o
    /// SendEnv=COLORTERM -o SendEnv=TERM_PROGRAM` and the arguments given;
    /// the variable that tells it unset before the user's files run (zsh,
    /// bash) or erased (fish); and the wrapper defined only where no `ssh`
    /// of the user's is. The real shells run it in `pty/shell_tests.rs`.
    #[test]
    fn each_script_wraps_ssh_only_when_told() {
        const CALL: &str = "command ssh -o SendEnv=COLORTERM -o SendEnv=TERM_PROGRAM";
        let zsh = include_str!("shell/zshenv.zsh");
        let bash = include_str!("shell/baud.bash");
        let fish = include_str!("shell/baud.fish");
        for (name, text, args) in [
            ("zsh", zsh, "\"$@\""),
            ("bash", bash, "\"$@\""),
            ("fish", fish, "$argv"),
        ] {
            assert_eq!(text.matches(CALL).count(), 1, "{name}: one call");
            assert!(
                text.contains(&format!("{CALL} {args}")),
                "{name}: the arguments given"
            );
        }
        // Told, and the telling gone before the user's files.
        let unset = |text: &str, set: &str, gone: &str| {
            let (s, g) = (text.find(set).expect(set), text.find(gone).expect(gone));
            s < g
        };
        assert!(unset(
            zsh,
            "__baud_ssh_env=${BAUD_SSH_ENV-}",
            "builtin unset BAUD_SSH_ENV"
        ));
        assert!(
            zsh.find("builtin unset BAUD_SSH_ENV") < zsh.find("source -- ${ZDOTDIR-$HOME}/.zshenv")
        );
        assert!(unset(
            bash,
            "__baud_ssh_env=${BAUD_SSH_ENV-}",
            "unset BAUD_SSH_ENV"
        ));
        assert!(bash.find("unset BAUD_SSH_ENV") < bash.find(". /etc/profile"));
        assert!(fish.contains("set -q __baud_ssh_env") && fish.contains("set -e __baud_ssh_env"));
        // The user's own kept.
        assert!(bash.contains("! declare -F ssh >/dev/null && ! alias ssh >/dev/null 2>&1"));
        assert!(fish.contains("not functions -q ssh"));
    }

    /// **Off, an unknown shell and macOS's `/bin/bash` get nothing**, and
    /// nothing is written for them.
    #[test]
    fn off_an_unknown_shell_and_macos_bash_get_nothing() {
        let d = scratch("none");
        let parent = env(&[("TMPDIR", d.to_str().expect("UTF-8"))]);
        assert_eq!(
            inject(
                Integration::Off,
                SshEnv::Off,
                "/bin/zsh",
                "/bin/zsh",
                &parent
            ),
            None
        );
        assert_eq!(
            inject(Integration::On, SshEnv::Off, "/bin/sh", "/bin/sh", &parent),
            None
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                inject(
                    Integration::On,
                    SshEnv::Off,
                    "/bin/bash",
                    "/bin/bash",
                    &parent
                ),
                None
            );
        }
        assert!(
            std::fs::read_dir(&d).expect("the dir").next().is_none(),
            "nothing written"
        );
        // The control: zsh on gets its directory.
        assert!(
            inject(
                Integration::On,
                SshEnv::Off,
                "/bin/zsh",
                "/bin/zsh",
                &parent
            )
            .is_some()
        );
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
