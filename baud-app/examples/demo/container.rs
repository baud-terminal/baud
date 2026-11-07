//! **The container the session ssh's into**: a Linux of six nixpkgs
//! packages -- busybox, bash, starship, OpenSSH's sshd, fastfetch and
//! btop -- run by
//! docker in a colima VM of its own on this Mac ([`COLIMA_PROFILE`]), its
//! host named [`HOST`] and its one user [`USER`]. fastfetch there prints
//! its whole default card (`nix/demo-container/fastfetch.jsonc` lists its
//! default modules): the VM's system as Linux in it sees it, which the
//! session's checks search for this machine's names; beside it it draws
//! baud's mark, a picture made from the repository's own logo file. btop
//! shows the cpu box alone (`nix/demo-container/btop.conf`), a graph for
//! each of the VM's CPUs, as many as this Mac's. The login shell is bash,
//! its prompt starship's, drawn from the repository's preset
//! (`assets/starship/baud.toml`) as `~ ❯`; it marks each prompt as
//! baud's shell does, shows the host as baud's badge while the session is
//! in, and clears the badge on the way out.
//!
//! **Made by Nix**: the image is the repository flake's `demo-container`
//! (`nix/demo-container/`), nixpkgs' `dockerTools.streamLayeredImage` at
//! the flake's pinned nixpkgs -- the revision `devenv.lock` holds, not the
//! demo's own [`crate::run::NIXPKGS`] -- with the packages for
//! aarch64-linux from cache.nixos.org, so no Linux builder is needed: they
//! come from the flake's `nixpkgs-demo` input, the revision
//! [`STARSHIP_NIXPKGS`] names, the starship the demo's own shell runs.
//! Its files are beside that expression, and this module reads the same
//! bytes (`include_str!`), so the tests below check what the image holds.
//!
//! **Pinned by digest**: the flake's output is a script that streams the
//! image, and the same nixpkgs streams the same bytes; [`IMAGE_SHA256`] is
//! their SHA-256. The stream is written to a file and refused, before
//! docker sees it, unless it hashes to the pin; then `docker load` reads
//! it.
//!
//! **Nothing of the person's ssh**: OpenSSH finds `~` through the passwd
//! database, not `HOME`, so the shell's `ssh` is given the scratch home's
//! config with `-F` ([`ZSH_SSH`]), which names its key, its known host and
//! no other file through `${HOME}`, the scratch home; no agent, no
//! multiplexing, no global known hosts. Both keys are made for the session
//! in the scratch directory, and the host key is known before the first
//! connection, so ssh never asks and never writes.
//!
//! **colima's own directories are left as they were found**: the session's
//! VM is a profile of its own, [`COLIMA_PROFILE`], in a directory made for
//! the session ([`ColimaHome`]), made with every CPU of this Mac and
//! docker's context left where it was, and deleted with its data and that
//! directory at the end. The container and its image are removed, and the
//! VM deleted, on every way out ([`Running`]'s drop), Ctrl-C among them: its
//! handler only sets [`INTERRUPTED`], and every wait the session makes stops
//! on it and returns, so the drop runs.

use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The container's host name and its one user.
pub const HOST: &str = "demo";
pub const USER: &str = "jordan";

/// **The nixpkgs revision starship is taken from**, for the shell on this
/// Mac and, through the flake's `nixpkgs-demo` input locked at the same
/// commit, for the container's: [`crate::run::NIXPKGS`] carries starship 1.26.0, which
/// advisories GHSA-679r-jwc5-r5m3 and GHSA-x486-6jjc-phfp cover, and this
/// nixpkgs-unstable revision carries 1.27.0, the first release outside
/// both. The container's six Linux packages come from it too
/// (`nix/demo-container/default.nix`), because the image's pin names their
/// store paths and at the flake's own nixpkgs btop and fastfetch have other
/// ones. The programs the demo's own shell runs still come from
/// [`crate::run::NIXPKGS`].
pub const STARSHIP_NIXPKGS: &str = "github:NixOS/nixpkgs/6dadaf4375be35033e1c84395adbd53ff5fded96";

/// **The image**, by the name and tag `nix/demo-container/default.nix`
/// gives it, which the session removes when it ends.
pub const IMAGE: &str = "baud-demo:readme";

/// The repository flake's output that streams the image.
pub const FLAKE_OUTPUT: &str = "demo-container";

/// **The image's stream, by SHA-256**: its packages from the flake's
/// `nixpkgs-demo` for `aarch64-linux`, the system colima's VM runs on an
/// Apple silicon Mac, and the script that streams them from the flake's
/// `nixpkgs`.
pub const IMAGE_SHA256: &str = "6ddfec1858299137e11eefb39ac153fb08755223c4fca2972609268fe3615fe9";

/// **Set by Ctrl-C**: the handler (`mac::catch_interrupt`) stores to it and
/// does nothing else, and the session's waits stop on it.
pub static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// `Err` once Ctrl-C has been pressed, for a wait to return through.
pub fn interrupted(flag: &AtomicBool) -> Result<(), String> {
    match flag.load(Ordering::Relaxed) {
        true => Err("interrupted by Ctrl-C".to_string()),
        false => Ok(()),
    }
}

/// The Linux the VM runs, for this Mac's processor; none is pinned but
/// Apple silicon's.
pub fn system() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("aarch64-linux"),
        other => Err(format!(
            "the container's image is pinned for aarch64-linux, the VM of an Apple silicon \
             Mac; this one is {other}"
        )),
    }
}

/// **The scratch home's ssh config**: the container on `port`, by the
/// alias [`HOST`], and every file through `${HOME}`, the scratch home.
pub fn ssh_config(port: u16) -> String {
    format!(
        "Host {HOST}\n  HostName 127.0.0.1\n  Port {port}\n  User {USER}\n  HostKeyAlias {HOST}\n  \
         IdentityFile ${{HOME}}/.ssh/{HOST}_ed25519\n  IdentitiesOnly yes\n  IdentityAgent none\n  \
         UserKnownHostsFile ${{HOME}}/.ssh/known_hosts\n  GlobalKnownHostsFile /dev/null\n  \
         StrictHostKeyChecking yes\n  UpdateHostKeys no\n  ControlMaster no\n  ControlPath none\n  \
         ForwardAgent no\n  ForwardX11 no\n  LogLevel ERROR\n"
    )
}

/// The shell's `ssh`, given the scratch config: zsh expands the `~`.
pub const ZSH_SSH: &str = "alias ssh='ssh -F ~/.ssh/config'\n";

/// Store paths and digests for the provenance file, each by its name.
pub type Tools = Vec<(String, String)>;

/// A flake output built: its store path and its [`Tools`].
pub type Built = Result<(PathBuf, Tools), String>;

/// The image, streamed and checked, and what the session needs beside it.
pub struct Image {
    /// The stream, written whole; removed once docker has loaded it.
    pub archive: PathBuf,
    /// GNU tar, which makes the keys' tars for `docker cp`.
    pub gnutar: PathBuf,
    /// For the provenance file: the stream's script and each package's
    /// store path, and the stream's SHA-256.
    pub tools: Tools,
}

/// **The image's stream**, `stream` run into `dir/image.tar` with its
/// progress kept off the terminal, and refused, the file removed, unless
/// it hashes to `pin`.
pub fn stream(
    dir: &Path,
    stream: &Path,
    pin: &str,
    sha256: &dyn Fn(&Path) -> Result<String, String>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let out = dir.join("image.tar");
    let file = fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let ran = Command::new(stream)
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{}: {e}", stream.display()))?;
    if !ran.status.success() {
        let _ = fs::remove_file(&out);
        return Err(format!(
            "the image's stream failed: {}",
            String::from_utf8_lossy(&ran.stderr).trim()
        ));
    }
    let got = sha256(&out)?;
    if got != pin {
        let _ = fs::remove_file(&out);
        return Err(format!(
            "the container's image streams to {got}, not the pinned {pin}"
        ));
    }
    Ok(out)
}

/// **The image**: the flake's [`FLAKE_OUTPUT`] built by `flake` -- its
/// packages substituted, the rest made on this Mac -- and its stream
/// checked against [`IMAGE_SHA256`] ([`stream`]); `nix_out` gives the GNU
/// tar the keys go in by.
pub fn image(
    dir: &Path,
    flake: &dyn Fn(&str) -> Built,
    nix_out: &dyn Fn(&str) -> Result<PathBuf, String>,
    sha256: &dyn Fn(&Path) -> Result<String, String>,
) -> Result<Image, String> {
    system()?;
    let (script, mut tools) = flake(FLAKE_OUTPUT)?;
    let archive = stream(dir, &script, IMAGE_SHA256, sha256)?;
    tools.push((
        "container image".to_string(),
        format!("sha256:{IMAGE_SHA256}"),
    ));
    let gnutar = nix_out("gnutar^out")?;
    tools.push((
        "gnutar^out".to_string(),
        gnutar.to_string_lossy().into_owned(),
    ));
    Ok(Image {
        archive,
        gnutar: gnutar.join("bin/tar"),
        tools,
    })
}

/// **A free port on the loopback**, the one the container's sshd is
/// published on.
pub fn free_port() -> Result<u16, String> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("a port: {e}"))?;
    l.local_addr()
        .map(|a| a.port())
        .map_err(|e| format!("a port: {e}"))
}

/// Files written into the scratch directory, each by its path under the
/// root, for the names check.
pub type Written = Vec<(String, Vec<u8>)>;

/// The keys a session makes: the host's, which the container gets, and the
/// user's, whose public half it gets; the scratch home holds the user's
/// and knows the host's.
pub struct Keys {
    pub host: PathBuf,
    pub user_pub: PathBuf,
}

/// **The session's two keys**, made by `keygen` with the comment
/// [`HOST`] -- ssh-keygen's default is the user and the host -- the user's
/// in the scratch home's `.ssh`, the host's in `dir`, and the scratch
/// home's known hosts naming it as [`HOST`]. Returns the files the scratch
/// home holds, for the names check.
pub fn keys(home: &Path, dir: &Path, keygen: &Path, port: u16) -> Result<(Keys, Written), String> {
    let ssh = home.join(".ssh");
    fs::create_dir_all(&ssh).map_err(|e| format!("{}: {e}", ssh.display()))?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let make = |at: &Path| -> Result<(), String> {
        let ok = Command::new(keygen)
            .args(["-q", "-t", "ed25519", "-N", "", "-C", HOST, "-f"])
            .arg(at)
            .stdin(Stdio::null())
            .status()
            .map_err(|e| format!("ssh-keygen: {e}"))?;
        if !ok.success() {
            return Err(format!("ssh-keygen did not make {}", at.display()));
        }
        Ok(())
    };
    let user = ssh.join(format!("{HOST}_ed25519"));
    let host = dir.join("ssh_host_ed25519_key");
    make(&user)?;
    make(&host)?;
    let host_pub =
        fs::read_to_string(host.with_extension("pub")).map_err(|e| format!("the host key: {e}"))?;
    let key: Vec<&str> = host_pub.split_whitespace().take(2).collect();
    let known = format!("{HOST} {}\n", key.join(" "));
    let config = ssh_config(port);
    fs::write(ssh.join("known_hosts"), &known).map_err(|e| format!("known_hosts: {e}"))?;
    fs::write(ssh.join("config"), &config).map_err(|e| format!("ssh config: {e}"))?;
    let user_pub = user.with_extension("pub");
    let public = fs::read(&user_pub).map_err(|e| format!("the user key: {e}"))?;
    let written = vec![
        ("home/.ssh/config".to_string(), config.into_bytes()),
        ("home/.ssh/known_hosts".to_string(), known.into_bytes()),
        (format!("home/.ssh/{HOST}_ed25519.pub"), public),
    ];
    Ok((Keys { host, user_pub }, written))
}

/// **The colima profile the session's VM is**, made for the session with
/// this Mac's every CPU, so btop there draws each of them, and deleted
/// with its data at the end: colima's own profiles, the default among
/// them, are neither started nor changed.
pub const COLIMA_PROFILE: &str = "baud-demo";

/// **`colima start`'s arguments for the session's VM**: [`COLIMA_PROFILE`]
/// with `cpus`, colima's defaults for the rest, docker's context not
/// switched to it, since the session talks to its socket alone, and
/// `~/.ssh/config` not touched: colima's default prepends an
/// `Include <its directory>/ssh_config` there, which for a directory of one
/// take would be a dangling line in that file for every take.
fn start_args(cpus: usize) -> Vec<String> {
    ["start", "-p", COLIMA_PROFILE, "--cpus"]
        .into_iter()
        .map(str::to_string)
        .chain([
            cpus.to_string(),
            "--activate=false".to_string(),
            "--ssh-config=false".to_string(),
        ])
        .collect()
}

/// Where each session's colima directory goes ([`ColimaHome`]).
const COLIMA_BASE: &str = "/tmp";

/// How each session's colima directory is named, before its pid.
const COLIMA_PREFIX: &str = "baud-colima-";

/// **The command that deletes a session's VM from the directory it is in.**
fn delete_command(home: &str) -> String {
    format!("COLIMA_HOME={home} LIMA_HOME={home}/_lima colima delete -p {COLIMA_PROFILE} -f --data")
}

/// What the end of a session says of its VM's deletion.
fn deleted_line(deleted: &Said, home: &str) -> String {
    match deleted {
        Ok(_) => "colima:  the session's VM deleted".to_string(),
        Err(_) => format!(
            "colima:  the session's VM deleted -- FAILED: run `{}`, then remove that directory",
            delete_command(home)
        ),
    }
}

/// **A session that did not end**, from the names in `base`: a directory of
/// a session is removed at its end, so one still there is a take killed past
/// the reach of its handler, whose VM may still run where a plain
/// `colima list` does not look, or a take running now. A second VM beside
/// it is not started; the sentence names the directory and how to delete
/// what is in it.
fn leftover(base: &Path, names: impl Iterator<Item = String>) -> Result<(), String> {
    let mut left: Vec<String> = names.filter(|n| n.starts_with(COLIMA_PREFIX)).collect();
    left.sort();
    let Some(first) = left.first() else {
        return Ok(());
    };
    let dir = base.join(first).to_string_lossy().into_owned();
    Err(format!(
        "{dir} is left by a session that did not end, or is in use by one running now, and its \
         VM may still run where a plain `colima list` does not look: run `{}`, then remove that \
         directory ({} found)",
        delete_command(&dir),
        left.len()
    ))
}

/// Whether `colima list --json`, a profile a line, names
/// [`COLIMA_PROFILE`].
fn listed(list: &str) -> bool {
    let name = format!("\"name\":\"{COLIMA_PROFILE}\"");
    list.lines().any(|l| l.contains(&name))
}

/// **The room kept for what lima puts under colima's directory**, in bytes:
/// a socket's path must fit `sun_path`, and the sockets of a running VM are
/// its own directory's and its network's, under this.
const SOCKET_ROOM: usize = 64;

/// The longest a Unix socket's path can be on macOS (`sun_path`).
const SOCKET_PATH: usize = 104;

/// **colima's directory for the session's VM**, every colima command run
/// with it: its config, lima's instance, the VM's disk and docker's socket
/// all go here, so no directory of colima's in the home is read, written or
/// made. colima 0.10.3 obeys `COLIMA_HOME` only when it names a directory
/// that exists (`os.Stat`, its `config/files.go`), and otherwise goes on to
/// `~/.colima`, which it makes if `XDG_CONFIG_HOME` is unset, and which once
/// it exists hides `$XDG_CONFIG_HOME/colima` and every VM there. So this is
/// made first. `LIMA_HOME` is named after the first command too, since
/// colima obeys one it inherits before its own directory's `_lima`. In
/// `/tmp`, physical, for the sockets:
/// the system's temporary directory leaves too little room. Dropped, it is
/// removed, unless [`ColimaHome::keep`] said the VM is still in it.
struct ColimaHome {
    dir: Option<PathBuf>,
}

impl ColimaHome {
    /// A new directory `baud-colima-<pid>-<n>` in `base`, this user's alone.
    fn new_in(base: &Path, pid: u32) -> Result<ColimaHome, String> {
        let base = fs::canonicalize(base).map_err(|e| format!("{}: {e}", base.display()))?;
        for n in 0..100 {
            let dir = base.join(format!("{COLIMA_PREFIX}{pid}-{n}"));
            if fs::DirBuilder::new().mode(0o700).create(&dir).is_ok() {
                let home = ColimaHome { dir: Some(dir) };
                if home.path().len() + SOCKET_ROOM > SOCKET_PATH {
                    return Err(format!(
                        "{} leaves lima too little room for a socket's path",
                        home.path()
                    ));
                }
                return Ok(home);
            }
        }
        Err(format!("no new directory in {}", base.display()))
    }

    fn path(&self) -> String {
        self.dir
            .as_deref()
            .map_or(String::new(), |d| d.to_string_lossy().into_owned())
    }

    /// What every colima command after the first is run with.
    fn env(&self) -> [(&'static str, String); 2] {
        let home = self.path();
        let lima = format!("{home}/_lima");
        [("COLIMA_HOME", home), ("LIMA_HOME", lima)]
    }

    /// **The first colima command**, told this directory and no `LIMA_HOME`,
    /// not even one it would inherit: colima makes and obeys a `LIMA_HOME`
    /// whichever directory it chose (`limaDir`, its `config/files.go`), so
    /// only without one does `_lima` say which it chose.
    fn list_command(&self) -> Command {
        let mut c = Command::new("colima");
        c.args(["list", "--json"])
            .env("COLIMA_HOME", self.path())
            .env_remove("LIMA_HOME")
            .stdin(Stdio::null());
        c
    }

    /// `cmd`, [`ColimaHome::list_command`], run, and what it listed, or why
    /// nothing may be started: it failed, or colima did not use this
    /// directory ([`ColimaHome::obeyed`]).
    fn first_list(&self, mut cmd: Command) -> Result<String, String> {
        let out = cmd.output().map_err(|e| format!("colima: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "colima list failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        self.obeyed()?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// **Whether colima is using this directory**: told no `LIMA_HOME`, a
    /// colima command makes `_lima` in the directory it chose, so after one,
    /// none here means it chose another.
    fn obeyed(&self) -> Result<(), String> {
        match self.dir.as_deref().map(|d| d.join("_lima").is_dir()) {
            Some(true) => Ok(()),
            _ => Err(format!(
                "colima did not use {}, the directory made for it, so nothing was started",
                self.path()
            )),
        }
    }

    /// The VM could not be deleted, so what is left of it stays here.
    fn keep(&mut self) {
        self.dir = None;
    }
}

impl Drop for ColimaHome {
    fn drop(&mut self) {
        if let Some(d) = &self.dir {
            let _ = fs::remove_dir_all(d);
        }
    }
}

/// What docker's context was before the session, the docker it talks to --
/// the session's VM's socket, empty until it is up -- and colima's directory
/// for the VM.
struct Colima {
    context: Option<String>,
    host: String,
    home: ColimaHome,
}

/// Whether `colima delete` took the session's VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gone {
    Yes,
    No,
}

/// A command's output, or why it failed.
type Said = Result<String, String>;

/// What runs a command: `say`, or a test's recorder.
type Runner<'a> = dyn FnMut(&str, &[&str], &[(&str, &str)]) -> Said + 'a;

fn say(cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Said {
    let out = Command::new(cmd)
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What the session started, and so must undo.
struct Undo {
    colima: Colima,
    name: String,
    image: String,
}

impl Undo {
    /// **Everything the session started, undone**, each command through
    /// `run`: the container removed, then its image, once the VM's docker
    /// answers; then the session's VM deleted with its data, however far
    /// its start got; then docker switched back to its context if anything
    /// moved it. Each is tried whatever the one before did. Says whether
    /// the VM is gone.
    fn run(&self, run: &mut Runner) -> Gone {
        if !self.colima.host.is_empty() {
            let host = [("DOCKER_HOST", self.colima.host.as_str())];
            let _ = run("docker", &["rm", "-f", &self.name], &host);
            let _ = run("docker", &["image", "rm", "-f", &self.image], &host);
        }
        let env = self.colima.home.env();
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let deleted = run(
            "colima",
            &["delete", "-p", COLIMA_PROFILE, "-f", "--data"],
            &env,
        );
        // Not `println!`: with its terminal closed, the session's drop still
        // has the rest of this to do.
        let line = deleted_line(&deleted, &self.colima.home.path());
        let _ = writeln!(std::io::stdout(), "{line}");
        let now = run("docker", &["context", "show"], &[]).ok();
        if let (Some(was), Some(now)) = (&self.colima.context, now) {
            if *was != now {
                let _ = run("docker", &["context", "use", was], &[]);
            }
        }
        match deleted {
            Ok(_) => Gone::Yes,
            Err(_) => Gone::No,
        }
    }
}

/// **The container, running**, and how to put everything back: dropped,
/// it undoes what the session started ([`Undo::run`]).
pub struct Running {
    undo: Undo,
}

impl Running {
    fn docker(&self, args: &[&str]) -> Said {
        say(
            "docker",
            args,
            &[("DOCKER_HOST", self.undo.colima.host.as_str())],
        )
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.undo.run(&mut say) == Gone::No {
            self.undo.colima.home.keep();
        }
    }
}

/// The socket colima's docker listens on, from `colima status --json`.
fn socket(status: &str) -> Option<String> {
    let key = "\"docker_socket\":\"";
    let at = status.find(key)? + key.len();
    let rest = &status[at..];
    Some(rest[..rest.find('"')?].to_string())
}

/// **The container up**, its sshd answering on `port`: the session's VM
/// started as [`COLIMA_PROFILE`] in its own directory ([`ColimaHome`]) with
/// every CPU of this Mac, the image loaded from its checked stream, the host
/// key and the user's public key copied in owned by root, and the container
/// started; then the banner read through colima's port forward. Nothing is
/// started while a directory of a session that did not end is there
/// ([`leftover`]), nor unless colima is using this one: it made `_lima` there
/// when told no `LIMA_HOME` and lists no profile in it yet.
/// Everything it started is undone when the result drops, and on an error
/// before that -- the undo exists before the VM's start, so a start
/// Ctrl-C cut short is deleted too. Ctrl-C ends the wait for the banner
/// ([`INTERRUPTED`]).
pub fn up(image: &Image, keys: &Keys, port: u16) -> Result<Running, String> {
    let tar = image.gnutar.as_path();
    let context = say("docker", &["context", "show"], &[]).ok();
    let base = fs::canonicalize(COLIMA_BASE).map_err(|e| format!("{COLIMA_BASE}: {e}"))?;
    let names = fs::read_dir(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    leftover(
        &base,
        names.filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned())),
    )?;
    let home = ColimaHome::new_in(&base, std::process::id())?;
    let owned_env = home.env();
    let env: Vec<(&str, &str)> = owned_env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let list = home
        .first_list(home.list_command())
        .map_err(|e| format!("{e}; the session's container runs in colima"))?;
    if listed(&list) {
        return Err(format!(
            "colima lists a profile {COLIMA_PROFILE} in {}, a directory made for this session, \
             so it is not reading that directory",
            home.path()
        ));
    }
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut run = Running {
        undo: Undo {
            colima: Colima {
                context,
                host: String::new(),
                home,
            },
            name: format!("baud-demo-{}", std::process::id()),
            image: IMAGE.to_string(),
        },
    };
    println!(
        "colima:  starting the session's VM, profile {COLIMA_PROFILE}, in {}, with this Mac's \
         {cpus} CPUs; it is deleted at the end",
        run.undo.colima.home.path()
    );
    let ok = Command::new("colima")
        .args(start_args(cpus))
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("colima: {e}"))?;
    interrupted(&INTERRUPTED)?;
    if !ok.success() {
        return Err("colima did not start the session's VM".to_string());
    }
    run.undo.colima.host = say("colima", &["status", "-p", COLIMA_PROFILE, "--json"], &env)
        .ok()
        .and_then(|s| socket(&s))
        .unwrap_or_default();
    if run.undo.colima.host.is_empty() {
        return Err("colima named no docker socket".to_string());
    }
    let archive = image.archive.to_string_lossy().into_owned();
    let loaded = run.docker(&["load", "--quiet", "--input", &archive]);
    // Docker holds the image now; a scratch directory kept after a failure
    // need not.
    let _ = fs::remove_file(&image.archive);
    if !loaded?
        .lines()
        .any(|l| l == format!("Loaded image: {IMAGE}"))
    {
        return Err(format!("docker loaded no {IMAGE}"));
    }
    let id = run.docker(&["image", "inspect", "--format", "{{.Id}}", IMAGE])?;
    println!("image:   {IMAGE}, {id}");
    let publish = format!("127.0.0.1:{port}:22");
    run.docker(&[
        "create",
        "--name",
        &run.undo.name,
        "--hostname",
        HOST,
        "--publish",
        &publish,
        IMAGE,
    ])?;
    copy_in(
        &run,
        tar,
        &keys.host,
        "ssh_host_ed25519_key",
        0o600,
        "/etc/ssh/",
    )?;
    copy_in(
        &run,
        tar,
        &keys.user_pub,
        "authorized_keys",
        0o644,
        &format!("/home/{USER}/.ssh/"),
    )?;
    run.docker(&["start", &run.undo.name])?;
    banner(port, &INTERRUPTED)?;
    println!("demo:    sshd answers on 127.0.0.1:{port}, in container {HOST}");
    Ok(run)
}

/// `file` into the container's `dir` as `name`, owned by root at `mode`,
/// through a tar on docker's input: `docker cp` of a file keeps the
/// user id it has on this Mac, which sshd refuses for a key.
fn copy_in(
    run: &Running,
    tar: &Path,
    file: &Path,
    name: &str,
    mode: u32,
    dir: &str,
) -> Result<(), String> {
    let stage = file.with_extension("in");
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    let to = stage.join(name);
    fs::copy(file, &to).map_err(|e| format!("{}: {e}", file.display()))?;
    fs::set_permissions(&to, fs::Permissions::from_mode(mode)).map_err(|e| format!("{e}"))?;
    let mut t = Command::new(tar)
        .args([
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "-cf",
            "-",
            "-C",
        ])
        .arg(&stage)
        .arg(name)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("tar: {e}"))?;
    let mut bytes = Vec::new();
    if let Some(mut o) = t.stdout.take() {
        o.read_to_end(&mut bytes).map_err(|e| format!("tar: {e}"))?;
    }
    let _ = t.wait();
    let target = format!("{}:{dir}", run.undo.name);
    let mut cp = Command::new("docker")
        .env("DOCKER_HOST", &run.undo.colima.host)
        .args(["cp", "-", &target])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("docker cp: {e}"))?;
    if let Some(mut i) = cp.stdin.take() {
        i.write_all(&bytes).map_err(|e| format!("docker cp: {e}"))?;
    }
    let ok = cp.wait().map_err(|e| format!("docker cp: {e}"))?;
    let _ = fs::remove_dir_all(&stage);
    match ok.success() {
        true => Ok(()),
        false => Err(format!("docker cp of {name} failed")),
    }
}

/// **sshd's banner through the port forward**, within 20 s or until
/// `stop` is set: lima forwards a published port to this Mac's loopback a
/// moment after the container starts.
fn banner(port: u16, stop: &AtomicBool) -> Result<(), String> {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) {
        interrupted(stop)?;
        if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut b = [0u8; 8];
            if s.read_exact(&mut b).is_ok() && &b == b"SSH-2.0-" {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!(
        "the container's sshd never answered on 127.0.0.1:{port}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // **The image's files**, the bytes `nix/demo-container/default.nix`
    // installs.

    /// **fastfetch's config**: baud's mark as the logo, drawn as a
    /// picture, and fastfetch's own default modules, its whole card.
    const FASTFETCH: &str = include_str!("../../../nix/demo-container/fastfetch.jsonc");

    /// **sshd's config**: the user alone, by key alone, with no PAM, no
    /// message of the day and no last login -- which would print the address
    /// the connection came from -- and nothing forwarded.
    const SSHD: &str = include_str!("../../../nix/demo-container/sshd_config");

    /// The login shell's start, bash's `/etc/profile`: the packages' tools
    /// on `PATH`, glibc's built-in `C.UTF-8` locale, starship's config and
    /// its cache, no history file, the badge set, and cleared on exit, then
    /// starship's prompt and the marks around it. sshd starts the session
    /// with an environment of its own, so the image's config could not set
    /// these.
    const PROFILE: &str = include_str!("../../../nix/demo-container/profile");

    const PASSWD: &str = include_str!("../../../nix/demo-container/passwd");
    const GROUP: &str = include_str!("../../../nix/demo-container/group");
    const SHADOW: &str = include_str!("../../../nix/demo-container/shadow");

    /// The modules fastfetch's config lists.
    fn modules(config: &str) -> Vec<String> {
        let at = config.find("\"modules\"").expect("a module list");
        let list = &config[at..];
        let list = &list[list.find('[').unwrap() + 1..list.find(']').unwrap()];
        list.split(',')
            .map(|m| m.trim().trim_matches('"').to_string())
            .filter(|m| !m.is_empty())
            .collect()
    }

    /// **fastfetch prints its whole default card**: the config lists
    /// fastfetch's own default modules, every one in its order
    /// (`[[source: fastfetch-2-60-0-structure-txt]]`), since a config file
    /// that sets the logo and lists no modules prints none
    /// (`[[source: fastfetch-2-60-0-jsonconfig-c]]`). The system's hardware
    /// among them is the VM's, as Linux in it sees it; what fastfetch
    /// printed is searched after the run for this machine's names and,
    /// given `--forbidden`, the list kept outside the repository. The
    /// control: the list with `cpu` taken out is caught.
    #[test]
    fn fastfetch_prints_its_whole_default_card() {
        const DEFAULT: &str = "Title:Separator:OS:Host:Kernel:Uptime:Packages:Shell:Display:\
            DE:WM:WMTheme:Theme:Icons:Font:Cursor:Terminal:TerminalFont:CPU:GPU:Memory:Swap:\
            Disk:LocalIp:Battery:PowerAdapter:Locale:Break:Colors";
        let default: Vec<String> = DEFAULT.split(':').map(str::to_lowercase).collect();
        assert_eq!(default.len(), 29);
        assert_eq!(modules(FASTFETCH), default);
        let less = FASTFETCH.replace("    \"cpu\",\n", "");
        assert_ne!(modules(&less), default, "the control");
    }

    /// **fastfetch draws baud's mark from its file, and clears nothing**:
    /// with its image logo `"iterm"` fastfetch reads the file and sends it
    /// whole in iTerm2's inline protocol, `OSC 1337 ; File=inline=1`, with
    /// no decoder of its own, which this fastfetch is built without
    /// (`[[source: fastfetch-2-60-0-image-c]]`); and with both a width and
    /// a height it writes the picture at the cursor, where with either
    /// missing it first clears the screen and its history, `CSI 2 J` and
    /// `CSI 3 J`. Not `"kitty-direct"`, which sends the file's path, a path
    /// on the container that baud cannot read; not `"auto"` or
    /// `"builtin"`, a distribution's mark. The control: the height taken
    /// out is caught.
    #[test]
    fn fastfetch_draws_baud_s_mark_whole() {
        let logo = |c: &str| -> Vec<String> {
            let at = c.find("\"logo\"").expect("a logo");
            let body = &c[at..];
            let body = &body[body.find('{').unwrap() + 1..body.find('}').unwrap()];
            body.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        };
        let set = logo(FASTFETCH);
        for line in [
            "\"type\": \"iterm\",",
            "\"source\": \"/usr/share/baud/mark.png\",",
            "\"preserveAspectRatio\": true",
        ] {
            assert!(set.iter().any(|l| l == line), "{line}: {set:?}");
        }
        let sized = |set: &[String]| {
            set.iter().any(|l| l.starts_with("\"width\": "))
                && set.iter().any(|l| l.starts_with("\"height\": "))
        };
        assert!(sized(&set));
        let bare = FASTFETCH.replace("\"height\"", "\"padding\"");
        assert!(!sized(&logo(&bare)), "the control");
        assert!(NIX.contains(
            "--output usr/share/baud/mark.png ${../../assets/logo/baud-mark-amber-tight.svg}"
        ));
    }

    /// **The container's prompt is starship's, marked as baud's shell
    /// marks its own, and its session shows the host as the badge**.
    /// starship sends no mark, and rewrites `PS1` before every prompt, so
    /// the marks go in after its `starship_precmd`, from a
    /// `PROMPT_COMMAND` entry of their own: `OSC 133 ; D` with the last
    /// command's status, which starship has saved by then in
    /// `STARSHIP_CMD_STATUS`, `$?` being its own, then `OSC 133 ; A ;` --
    /// baud's own mark, the semicolon kept, so `session::PROMPT_MARK`
    /// counts it. Both are printed, not put in `PS1`: readline draws the
    /// prompt again when the first key arrives, and a mark in it would be
    /// counted twice. `OSC 133 ; B`, where the input begins, ends `PS1`
    /// inside bash's `\[` and `\]`, so readline counts none of it as
    /// columns, and `OSC 133 ; C` ends `PS0`, after starship's own, as the
    /// command starts. Without the `D` and the `B` baud reads the next
    /// `A`, the command's output between, as the shell drawing its prompt
    /// again and erases back to it (`prompt.rs`), which took fastfetch's
    /// card. The login sets the badge to the host, iTerm2's
    /// `SetBadgeFormat` of its base64, and the shell's exit sets it empty,
    /// so it is gone once ssh returns. The control: a mark without its
    /// semicolon is not counted.
    #[test]
    fn the_container_s_prompt_is_marked_and_badged() {
        let lines: Vec<&str> = PROFILE.lines().collect();
        let at = |l: &str| {
            lines
                .iter()
                .position(|x| *x == l)
                .unwrap_or_else(|| panic!("{l}"))
        };
        let init = at("eval \"$(starship init bash)\"");
        assert!(at("export STARSHIP_CONFIG=/etc/starship.toml") < init);
        assert!(init < at("PROMPT_COMMAND=\"$PROMPT_COMMAND; marks\""));
        assert!(init < at("PS0=\"$PS0\"'\\e]133;C\\a'"));
        assert!(
            PROFILE.contains(
                "marks() {\n  printf '\\033]133;D;%s\\a\\033]133;A;\\a' \"$STARSHIP_CMD_STATUS\"\n  \
                 PS1=\"$PS1\"'\\[\\e]133;B\\a\\]'\n}\n"
            ),
            "{PROFILE}"
        );
        let mark = "\x1b]133;A;\x07";
        assert_eq!(crate::session::marks(mark.as_bytes()), 1);
        assert_eq!(crate::session::marks(b"\x1b]133;A\x07"), 0, "the control");
        let badge = format!("printf '\\033]1337;SetBadgeFormat={}\\a'", base64(HOST));
        assert!(PROFILE.lines().any(|l| l == badge), "{badge}");
        assert!(
            PROFILE
                .lines()
                .any(|l| l == "trap 'printf \"\\033]1337;SetBadgeFormat=\\a\"' EXIT")
        );
        assert!(PROFILE.lines().any(|l| l == "unset HISTFILE"));
    }

    /// Standard base64 with padding, for the badge.
    fn base64(s: &str) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in s.as_bytes().chunks(3) {
            let n = c
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..4 {
                out.push(match i <= c.len() {
                    true => char::from(A[(n >> (18 - 6 * i) & 63) as usize]),
                    false => '=',
                });
            }
        }
        out
    }

    /// **sshd prints nothing of the connection and lets in the user
    /// alone, by key**, no account having a password; the user's shell is
    /// the image's bash, in glibc's `C.UTF-8`, and its prompt the one the
    /// font check draws.
    #[test]
    fn sshd_admits_the_user_by_key_and_prints_no_address() {
        for line in [
            "PrintLastLog no",
            "PrintMotd no",
            "PasswordAuthentication no",
            "KbdInteractiveAuthentication no",
            "UsePAM no",
            "AllowUsers jordan",
            "PermitRootLogin no",
        ] {
            assert!(SSHD.lines().any(|l| l == line), "{line}");
        }
        assert!(
            PASSWD
                .lines()
                .any(|l| l == format!("{USER}:x:1000:1000:{USER}:/home/{USER}:/usr/bin/bash"))
        );
        assert!(GROUP.lines().any(|l| l == format!("{USER}:x:1000:")));
        // No account has a password to log in by.
        assert!(SHADOW.lines().all(|l| l.split(':').nth(1) == Some("*")));
        assert!(PROFILE.lines().any(|l| l == "export LANG=C.UTF-8"));
        assert!(
            NIX.lines()
                .any(|l| l.trim() == "ln -s ${bashInteractive}/bin/bash usr/bin/bash")
        );
        assert_eq!(
            crate::session::REMOTE_PROMPT,
            "~ \u{276F} ",
            "starship's, at home"
        );
    }

    /// **ssh reads only the scratch home**: every file through `${HOME}`,
    /// which ssh expands from the environment where `~` would be the
    /// passwd database's home, no agent, no global known hosts, no
    /// multiplexing -- whose client holds no connection of its own, so CD
    /// would not light -- and the host key checked against the known host
    /// written for it. The shell's `ssh` takes this config. The control: a
    /// config naming `~/.ssh` is caught.
    #[test]
    fn ssh_reads_only_the_scratch_home() {
        let c = ssh_config(40022);
        for line in [
            "Host demo",
            "  HostName 127.0.0.1",
            "  Port 40022",
            "  User jordan",
            "  HostKeyAlias demo",
            "  IdentityFile ${HOME}/.ssh/demo_ed25519",
            "  IdentitiesOnly yes",
            "  IdentityAgent none",
            "  UserKnownHostsFile ${HOME}/.ssh/known_hosts",
            "  GlobalKnownHostsFile /dev/null",
            "  StrictHostKeyChecking yes",
            "  ControlMaster no",
            "  ControlPath none",
            "  ForwardAgent no",
        ] {
            assert!(c.lines().any(|l| l == line), "{line}:\n{c}");
        }
        let tilde = |s: &str| s.contains('~');
        assert!(!tilde(&c));
        assert!(tilde(&c.replace("${HOME}", "~")), "the control");
        assert_eq!(ZSH_SSH, "alias ssh='ssh -F ~/.ssh/config'\n");
    }

    /// The expression that makes the image, and the flake that names it.
    const NIX: &str = include_str!("../../../nix/demo-container/default.nix");
    const FLAKE: &str = include_str!("../../../flake.nix");

    /// **The image's files**, as `nix/demo-container/default.nix` writes
    /// them: each file this module and btop's check read installed from the
    /// same file, the mark's picture readable by all, sshd's directories
    /// root's and not writable by others, the shadow file root's alone, the
    /// temporary directory sticky, each package linked once, sshd the entry
    /// point, and the image named [`IMAGE`] and built for arm64 by the
    /// flake's [`FLAKE_OUTPUT`] on this Mac.
    /// The control: the expression with the shadow file's mode widened.
    #[test]
    fn the_image_holds_what_sshd_and_fastfetch_need() {
        let has = |nix: &str, line: &str| nix.lines().any(|l| l.trim() == line);
        let (name, tag) = IMAGE.split_once(':').unwrap();
        for line in [
            format!("name = \"{name}\";"),
            format!("tag = \"{tag}\";"),
            "architecture = \"arm64\";".to_string(),
            "install -d -m 0700 root".to_string(),
            "install -d -m 1777 tmp".to_string(),
            "usr usr/bin usr/sbin usr/share usr/share/baud var var/empty".to_string(),
            "install -m 0644 ${./passwd} etc/passwd".to_string(),
            "install -m 0644 ${./group} etc/group".to_string(),
            "install -m 0600 ${./shadow} etc/shadow".to_string(),
            "install -m 0644 ${./profile} etc/profile".to_string(),
            "install -m 0644 ${./sshd_config} etc/ssh/sshd_config".to_string(),
            "install -m 0644 ${starshipConfig} etc/starship.toml".to_string(),
            "install -m 0644 ${./fastfetch.jsonc} home/jordan/.config/fastfetch/config.jsonc"
                .to_string(),
            "install -m 0644 ${./btop.conf} home/jordan/.config/btop/btop.conf".to_string(),
            "chmod 0644 usr/share/baud/mark.png".to_string(),
            "ln -s ${busybox}/bin bin".to_string(),
            "ln -s ${bashInteractive}/bin/bash usr/bin/bash".to_string(),
            "ln -s ${starship}/bin/starship usr/bin/starship".to_string(),
            "chown 1000:1000 home/jordan".to_string(),
            "ln -s ${fastfetch-unwrapped}/bin/fastfetch usr/bin/fastfetch".to_string(),
            "ln -s ${btop}/bin/btop usr/bin/btop".to_string(),
            "ln -s ${openssh}/bin/sshd usr/sbin/sshd".to_string(),
            "\"/usr/sbin/sshd\"".to_string(),
        ] {
            assert!(has(NIX, &line), "{line}");
        }
        assert!(NIX.contains("install -d -m 0755 etc etc/ssh home home/jordan"));
        assert!(!NIX.contains("home/baud"), "the user's home is jordan's");
        assert!(NIX.contains(
            "inherit (linux) busybox openssh fastfetch-unwrapped btop starship bashInteractive;"
        ));
        let wide = NIX.replace("-m 0600 ${./shadow}", "-m 0644 ${./shadow}");
        assert!(
            !has(&wide, "install -m 0600 ${./shadow} etc/shadow"),
            "the control"
        );
        assert!(FLAKE.contains(&format!(
            "aarch64-darwin.{FLAKE_OUTPUT} = import ./nix/demo-container {{"
        )));
        assert!(FLAKE.contains("linux = nixpkgs-demo.legacyPackages.aarch64-linux;"));
        assert_eq!(
            socket(r#"{"arch":"aarch64","docker_socket":"unix:///x/docker.sock","k":1}"#),
            Some("unix:///x/docker.sock".to_string())
        );
    }

    /// **The image's prompt is the preset with only what it shows over
    /// ssh in docker turned off**: the user and the host, which starship
    /// shows over ssh on a line of their own, and the container's name,
    /// which it shows in docker, so the prompt there is `~ ❯` and the
    /// session's badge alone names the server; nothing else of
    /// `assets/starship/baud.toml` is changed, the file read whole by
    /// nix. Every package of the image comes from the flake's `nixpkgs-demo`,
    /// locked at the commit the demo's own starship is built from. The
    /// control: a fourth module turned off is caught.
    #[test]
    fn the_image_s_prompt_is_the_preset_with_the_ssh_lines_off() {
        let off = |nix: &str| -> Vec<String> {
            let at = nix
                .find("pkgs.lib.importTOML ../../assets/starship/baud.toml) {")
                .unwrap();
            let body = &nix[at..];
            let body = &body[body.find('{').unwrap() + 1..body.find('}').unwrap()];
            body.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        };
        assert_eq!(
            off(NIX),
            [
                "username.disabled = true;",
                "hostname.disabled = true;",
                "container.disabled = true;"
            ]
        );
        let more = NIX.replace(
            "container.disabled = true;",
            "container.disabled = true;\n      status.disabled = true;",
        );
        assert_eq!(off(&more).len(), 4, "the control");
        let pin = STARSHIP_NIXPKGS
            .strip_prefix("github:NixOS/nixpkgs/")
            .unwrap();
        assert!(FLAKE.contains(&format!(
            "nixpkgs-demo.url = \"github:NixOS/nixpkgs/{pin}\";"
        )));
        let lock = include_str!("../../../flake.lock");
        let node = &lock[lock
            .find("\"nixpkgs-demo\": {")
            .expect("the input is locked")..];
        let node = &node[..node.find("\"original\"").unwrap()];
        assert!(node.contains(&format!("\"rev\": \"{pin}\"")), "{node}");
    }

    /// A scratch directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("demo-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// **A stream that hashes to another is refused before docker sees
    /// it**, and its file removed; so is one whose script fails. The
    /// control: the same stream at its own pin is kept.
    #[test]
    fn a_stream_that_is_not_the_pinned_one_is_refused() {
        let d = scratch("stream");
        let script = d.join("stream");
        fs::write(&script, "#!/bin/sh\nprintf layer\necho progress >&2\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let sha = crate::provenance::sha256;
        // The SHA-256 of `layer`, and of `layes`.
        let own = "dac1d7cfa95021764849fd102524e141488c5e3a90f861dbb5a12d9ac8584f85";
        let pin = "1d3134b92234c4700686fe5768539ddf501062d1f18f9d7630c4ad5fd0e40030";
        let refused = stream(&d.join("a"), &script, pin, &sha).unwrap_err();
        assert!(refused.contains("not the pinned"), "{refused}");
        assert!(!d.join("a/image.tar").exists());
        let kept = stream(&d.join("b"), &script, own, &sha).unwrap();
        assert_eq!(fs::read(&kept).unwrap(), b"layer");
        let fails = d.join("fails");
        fs::write(&fails, "#!/bin/sh\necho no >&2\nexit 1\n").unwrap();
        fs::set_permissions(&fails, fs::Permissions::from_mode(0o755)).unwrap();
        let failed = stream(&d.join("c"), &fails, own, &sha).unwrap_err();
        assert!(failed.contains("stream failed: no"), "{failed}");
        assert!(!d.join("c/image.tar").exists());
        let _ = fs::remove_dir_all(&d);
    }

    /// The commands [`Undo::run`] gives, each with the `DOCKER_HOST` it
    /// names, the VM's docker at `host`, `context show` answering `now`
    /// and the container's removal failing.
    fn undone(host: &str, was: &str, now: &str) -> Vec<String> {
        let u = Undo {
            colima: Colima {
                context: Some(was.to_string()),
                host: host.to_string(),
                home: ColimaHome {
                    dir: Some(PathBuf::from("/c/home")),
                },
            },
            name: "baud-demo-7".to_string(),
            image: IMAGE.to_string(),
        };
        let mut calls = Vec::new();
        u.run(&mut |cmd, args, env| {
            let host: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v} ")).collect();
            calls.push(format!("{}{cmd} {}", host.concat(), args.join(" ")));
            match args {
                ["rm", ..] => Err("no such container".to_string()),
                ["context", "show"] => Ok(now.to_string()),
                _ => Ok(String::new()),
            }
        });
        calls
    }

    /// **Every way out undoes what the session started**, the drop and
    /// Ctrl-C's alike, since Ctrl-C returns through the drop: the container
    /// and then its image removed from the VM's docker, the image though
    /// the container's removal failed; the session's VM deleted with its
    /// data, in colima's directory for it, and before its docker answered
    /// only that; and docker's context put back only if it moved. No other
    /// profile is named: none is stopped. The VM's start names its own
    /// profile, every CPU asked for, and leaves docker's context and
    /// `~/.ssh/config`; a profile of its name already listed is found, and
    /// the control, another's, is not. Perturbation: pass the delete no
    /// environment, and colima looks for the VM in the home.
    #[test]
    fn the_undo_removes_the_container_and_deletes_the_vm() {
        let rm = "DOCKER_HOST=unix:///c/docker.sock docker rm -f baud-demo-7";
        let rmi = "DOCKER_HOST=unix:///c/docker.sock docker image rm -f baud-demo:readme";
        let delete =
            "COLIMA_HOME=/c/home LIMA_HOME=/c/home/_lima colima delete -p baud-demo -f --data";
        assert_eq!(
            undone("unix:///c/docker.sock", "default", "colima-baud-demo"),
            [
                rm,
                rmi,
                delete,
                "docker context show",
                "docker context use default"
            ]
        );
        assert_eq!(
            undone("unix:///c/docker.sock", "default", "default"),
            [rm, rmi, delete, "docker context show"]
        );
        assert_eq!(
            undone("", "default", "default"),
            [delete, "docker context show"],
            "a start cut short"
        );
        assert_eq!(
            start_args(18),
            [
                "start",
                "-p",
                "baud-demo",
                "--cpus",
                "18",
                "--activate=false",
                "--ssh-config=false"
            ]
        );
        let list = "{\"name\":\"default\",\"status\":\"Stopped\"}\n";
        assert!(!listed(list), "the control");
        assert!(listed(&format!(
            "{list}{{\"name\":\"baud-demo\",\"status\":\"Running\"}}\n"
        )));
    }

    /// **colima's directory exists before colima is asked**, since 0.10.3
    /// goes on to `~/.colima` from a `COLIMA_HOME` that does not exist; it
    /// is this user's alone and physical through a symbolic link, names
    /// itself and lima's directory in the environment, leaves a socket room,
    /// and a second one in the same place is another. Dropped, it is gone,
    /// and one told to keep what is in it stays. `obeyed` reads the `_lima`
    /// a colima command makes: without it, colima chose another directory.
    /// Perturbation: name the path without making it, and the first assert
    /// fails, which is the run that made `~/.colima`.
    #[test]
    fn colima_s_directory_is_made_first_and_taken_away_last() {
        // In /tmp, as the real one is: the temporary directory is too long.
        let short = |tag: &str| {
            let d = PathBuf::from(format!("/tmp/c-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            d
        };
        let real = short("r");
        let link = short("l").join("b");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let home = ColimaHome::new_in(&link, 42).unwrap();
        let dir = PathBuf::from(home.path());
        assert!(dir.is_dir(), "there before colima is asked");
        assert_eq!(dir, real.canonicalize().unwrap().join("baud-colima-42-0"));
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert_eq!(
            home.env(),
            [
                ("COLIMA_HOME", home.path()),
                ("LIMA_HOME", format!("{}/_lima", home.path()))
            ]
        );
        assert!(home.obeyed().is_err(), "no command has used it");
        fs::create_dir(dir.join("_lima")).unwrap();
        assert_eq!(home.obeyed(), Ok(()));
        let mut second = ColimaHome::new_in(&link, 42).unwrap();
        assert_ne!(second.path(), home.path());
        let left = PathBuf::from(second.path());
        second.keep();
        drop(second);
        assert!(left.is_dir(), "the VM could not be deleted");
        drop(home);
        assert!(!dir.exists());
        let longest = format!("/private/tmp/{COLIMA_PREFIX}{}-99", u32::MAX);
        assert!(longest.len() + SOCKET_ROOM <= SOCKET_PATH);
        let far = real.join("x".repeat(100));
        fs::create_dir_all(&far).unwrap();
        assert!(
            ColimaHome::new_in(&far, 42).is_err(),
            "a long directory leaves too little room"
        );
        for d in [real, link.parent().unwrap().to_path_buf()] {
            let _ = fs::remove_dir_all(d);
        }
    }

    /// **A session that did not end is refused, by name, with the command
    /// that deletes it**, the first of them when there are several; what else
    /// is in the directory is not a session's. Perturbation: find nothing,
    /// and a second VM starts beside the one a killed take left.
    #[test]
    fn a_session_that_did_not_end_is_refused_by_name() {
        let base = Path::new("/private/tmp");
        let names = |n: &[&str]| {
            n.iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .into_iter()
        };
        assert_eq!(leftover(base, names(&[])), Ok(()));
        assert_eq!(
            leftover(
                base,
                names(&["baud-colima", "baud-demo-7-0", "x-baud-colima-7-0"])
            ),
            Ok(()),
            "none is a session's"
        );
        let said = leftover(
            base,
            names(&["other", "baud-colima-9-0", "baud-colima-10-0"]),
        );
        let said = said.unwrap_err();
        assert!(
            said.starts_with("/private/tmp/baud-colima-10-0 is left by"),
            "{said}"
        );
        assert!(
            said.contains(
                "run `COLIMA_HOME=/private/tmp/baud-colima-10-0 \
                 LIMA_HOME=/private/tmp/baud-colima-10-0/_lima colima delete -p baud-demo -f \
                 --data`, then remove"
            ),
            "{said}"
        );
        assert!(said.contains("(2 found)"), "{said}");
    }

    /// **The line a failed delete prints can be pasted**: the command whole,
    /// one space between its words, and the directory it names. Perturbation:
    /// a string continued without its backslash, as this was, holds the
    /// source's indent in the middle of the command.
    #[test]
    fn a_failed_delete_prints_a_command_that_can_be_pasted() {
        assert_eq!(
            deleted_line(&Err("no".to_string()), "/h"),
            "colima:  the session's VM deleted -- FAILED: run `COLIMA_HOME=/h LIMA_HOME=/h/_lima \
             colima delete -p baud-demo -f --data`, then remove that directory"
        );
        assert_eq!(
            deleted_line(&Ok(String::new()), "/h"),
            "colima:  the session's VM deleted"
        );
    }

    /// **The first colima command is told this directory and no `LIMA_HOME`**,
    /// taking out one it would inherit, and it is the only one that is.
    /// Perturbation: name `LIMA_HOME` on it, and with colima ignoring this
    /// directory `_lima` appears here anyway, so `obeyed` could never fire.
    #[test]
    fn the_first_list_is_told_no_lima_home() {
        let home = ColimaHome {
            dir: Some(PathBuf::from("/c/home")),
        };
        let c = home.list_command();
        let envs: Vec<(String, Option<String>)> = c
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(envs.contains(&("LIMA_HOME".to_string(), None)), "{envs:?}");
        assert!(
            envs.contains(&("COLIMA_HOME".to_string(), Some("/c/home".to_string()))),
            "{envs:?}"
        );
        assert_eq!(
            c.get_args().collect::<Vec<_>>(),
            ["list", "--json"],
            "reads, starts nothing"
        );
        assert!(
            home.env().iter().any(|(k, _)| *k == "LIMA_HOME"),
            "the rest are"
        );
    }

    /// **colima itself refuses to be believed unless it used the directory**,
    /// through its real `list`, under a scratch `HOME`, `XDG_CONFIG_HOME`
    /// unset: a directory that is missing, which colima goes past to
    /// `~/.colima`, is refused, and that `~/.colima` is the scratch home's; a
    /// directory that exists is used. The control, why the first command is
    /// told no `LIMA_HOME`: given one, the missing directory reads as used.
    /// A host with no colima is skipped, saying so.
    #[test]
    fn colima_is_not_believed_unless_it_used_the_directory() {
        let d = scratch("colima-obeyed");
        let scratch_home = d.join("h");
        fs::create_dir_all(&scratch_home).unwrap();
        let found = Command::new("colima")
            .arg("version")
            .env("HOME", &scratch_home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("COLIMA_HOME")
            .env_remove("LIMA_HOME")
            .output();
        if found.is_err() {
            println!("skipped: no colima on PATH");
            let _ = fs::remove_dir_all(&d);
            return;
        }
        let in_scratch = |home: &ColimaHome| {
            let mut c = home.list_command();
            c.env("HOME", &scratch_home).env_remove("XDG_CONFIG_HOME");
            c
        };
        let missing = ColimaHome {
            dir: Some(d.join("missing")),
        };
        let said = missing.first_list(in_scratch(&missing)).unwrap_err();
        assert!(said.contains("did not use"), "{said}");
        assert!(scratch_home.join(".colima/_lima").is_dir(), "where it went");
        let mut told = in_scratch(&missing);
        told.env("LIMA_HOME", d.join("missing/_lima"));
        assert!(
            missing.first_list(told).is_ok(),
            "the check that cannot fire"
        );
        let made = d.join("made");
        fs::create_dir_all(&made).unwrap();
        let made = ColimaHome { dir: Some(made) };
        assert!(made.first_list(in_scratch(&made)).is_ok());
        drop((missing, made));
        let _ = fs::remove_dir_all(&d);
    }

    /// **The wait for sshd stops on Ctrl-C** at once, with nothing
    /// listening, rather than after its 20 s; the control: unset, it reads
    /// a banner.
    #[test]
    fn the_wait_for_sshd_stops_on_ctrl_c() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let serve = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.write_all(b"SSH-2.0-test\r\n").unwrap();
        });
        let t = Instant::now();
        let stopped = banner(free_port().unwrap(), &AtomicBool::new(true)).unwrap_err();
        assert_eq!(stopped, "interrupted by Ctrl-C");
        assert!(t.elapsed() < Duration::from_secs(1));
        banner(port, &AtomicBool::new(false)).unwrap();
        serve.join().unwrap();
    }
}
