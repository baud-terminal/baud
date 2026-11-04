//! **The container the session ssh's into**: a Linux of three nixpkgs
//! packages -- busybox, OpenSSH's sshd and fastfetch -- run by docker in
//! colima's VM on this Mac, its host named [`HOST`] and its one user
//! [`USER`]. fastfetch there shows the container, never this Mac: its
//! modules are listed (`nix/demo-container/fastfetch.jsonc`), and none of
//! them reads the chip, the memory, a disk, an address, the battery, the
//! uptime or a user's name.
//!
//! **Made by Nix**: the image is the repository flake's `demo-container`
//! (`nix/demo-container/`), nixpkgs' `dockerTools.streamLayeredImage` at
//! the flake's pinned nixpkgs -- the revision `devenv.lock` holds, not the
//! demo's own [`crate::run::NIXPKGS`] -- with the packages for
//! aarch64-linux from cache.nixos.org, so no Linux builder is needed.
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
//! **colima is left as it was found**: started if it was stopped, with no
//! flags, so its profile is not rewritten, and stopped again at the end;
//! the docker context it switches to is switched back. The container and
//! its image are removed on every way out ([`Running`]'s drop), Ctrl-C
//! among them: its handler only sets [`INTERRUPTED`], and every wait the
//! session makes stops on it and returns, so the drop runs.

use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The container's host name and its one user.
pub const HOST: &str = "demo";
pub const USER: &str = "baud";

/// **The image**, by the name and tag `nix/demo-container/default.nix`
/// gives it, which the session removes when it ends.
pub const IMAGE: &str = "baud-demo:readme";

/// The repository flake's output that streams the image.
pub const FLAKE_OUTPUT: &str = "demo-container";

/// **The image's stream, by SHA-256**, from the flake's pinned nixpkgs for
/// `aarch64-linux`, the system colima's VM runs on an Apple silicon Mac.
pub const IMAGE_SHA256: &str = "889abf85eae143fa635b389106eacbcf8029948f2f27786fe9f7a58a353f87fb";

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

/// What colima and docker were before the session, and the docker it
/// talks to.
struct Colima {
    started: bool,
    context: Option<String>,
    host: String,
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
    /// `run`: the container removed, then its image, then colima stopped
    /// if it was stopped before, then docker switched back to its context
    /// if colima moved it. Each is tried whatever the one before did.
    fn run(&self, run: &mut Runner) {
        let host = [("DOCKER_HOST", self.colima.host.as_str())];
        let _ = run("docker", &["rm", "-f", &self.name], &host);
        let _ = run("docker", &["image", "rm", "-f", &self.image], &host);
        if self.colima.started {
            let stopped = run("colima", &["stop"], &[]);
            println!(
                "colima:  stopped again{}",
                if stopped.is_ok() {
                    ""
                } else {
                    " -- FAILED: run `colima stop`"
                }
            );
        }
        let now = run("docker", &["context", "show"], &[]).ok();
        if let (Some(was), Some(now)) = (&self.colima.context, now) {
            if *was != now {
                let _ = run("docker", &["context", "use", was], &[]);
            }
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
        self.undo.run(&mut say);
    }
}

/// The socket colima's docker listens on, from `colima status --json`.
fn socket(status: &str) -> Option<String> {
    let key = "\"docker_socket\":\"";
    let at = status.find(key)? + key.len();
    let rest = &status[at..];
    Some(rest[..rest.find('"')?].to_string())
}

/// **The container up**, its sshd answering on `port`: colima started if
/// it was stopped, the image loaded from its checked stream, the host key
/// and the user's public key copied in owned by root, and the container
/// started; then the banner read through colima's port forward. Everything
/// it started is undone when the result drops, and on an error before
/// that. Ctrl-C ends the wait for the banner ([`INTERRUPTED`]).
pub fn up(image: &Image, keys: &Keys, port: u16) -> Result<Running, String> {
    let tar = image.gnutar.as_path();
    let context = say("docker", &["context", "show"], &[]).ok();
    let was = Command::new("colima")
        .arg("status")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("colima: {e}; the session's container runs in colima"))?;
    let started = !was.success();
    if started {
        println!("colima:  stopped; starting it for the session, and stopping it after");
        let ok = Command::new("colima")
            .arg("start")
            .status()
            .map_err(|e| format!("colima: {e}"))?;
        if !ok.success() {
            let _ = Command::new("colima").arg("stop").status();
            return Err("colima did not start".to_string());
        }
    }
    let host = say("colima", &["status", "--json"], &[])
        .ok()
        .and_then(|s| socket(&s));
    let run = Running {
        undo: Undo {
            colima: Colima {
                started,
                context,
                host: host.unwrap_or_default(),
            },
            name: format!("baud-demo-{}", std::process::id()),
            image: IMAGE.to_string(),
        },
    };
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
        "/home/baud/.ssh/",
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

    /// **fastfetch's modules, every one named**: the user and host, the
    /// kernel the VM boots, the grid and the terminal's two colours -- which
    /// fastfetch asks the terminal for, so baud answers a query nobody typed
    /// -- and the palette. Tux is drawn beside them: the image has no
    /// `os-release` naming a distribution, so no other logo is true of it.
    const FASTFETCH: &str = include_str!("../../../nix/demo-container/fastfetch.jsonc");

    /// **sshd's config**: the user alone, by key alone, with no PAM, no
    /// message of the day and no last login -- which would print the address
    /// the connection came from -- and nothing forwarded.
    const SSHD: &str = include_str!("../../../nix/demo-container/sshd_config");

    /// The login shell's start: the packages' tools on `PATH`, glibc's
    /// built-in `C.UTF-8` locale, and the prompt `session::REMOTE_PROMPT`
    /// waits for. sshd starts the session with an environment of its own, so
    /// the image's config could not set these.
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

    /// **fastfetch shows nothing of this Mac**: it reads every module it
    /// is told to and no other, so the list is the whole of what it can
    /// print, and none on it reads the chip, the memory, a disk, an
    /// address, the battery, the uptime, a display, the users or the
    /// packages -- what the VM would pass up from the Mac. The control:
    /// the list with `cpu` added.
    #[test]
    fn fastfetch_shows_nothing_of_the_mac() {
        let shown = [
            "title",
            "separator",
            "kernel",
            "terminalsize",
            "terminaltheme",
            "break",
            "colors",
        ];
        assert_eq!(modules(FASTFETCH), shown);
        let machine = [
            "cpu", "gpu", "memory", "swap", "disk", "host", "board", "localip", "publicip",
            "battery", "uptime", "display", "users", "packages", "wifi", "dns", "netio",
        ];
        let reads = |list: &[String]| -> Vec<String> {
            list.iter()
                .filter(|m| machine.contains(&m.as_str()))
                .cloned()
                .collect()
        };
        assert_eq!(reads(&modules(FASTFETCH)), Vec::<String>::new());
        let more = FASTFETCH.replace("\"kernel\",", "\"kernel\",\n    \"cpu\",");
        assert_eq!(reads(&modules(&more)), ["cpu"]);
    }

    /// **sshd prints nothing of the connection and lets in the user
    /// alone, by key**, no account having a password; the shell's prompt
    /// is the one the session awaits, in glibc's `C.UTF-8`.
    #[test]
    fn sshd_admits_the_user_by_key_and_prints_no_address() {
        for line in [
            "PrintLastLog no",
            "PrintMotd no",
            "PasswordAuthentication no",
            "KbdInteractiveAuthentication no",
            "UsePAM no",
            "AllowUsers baud",
            "PermitRootLogin no",
        ] {
            assert!(SSHD.lines().any(|l| l == line), "{line}");
        }
        assert!(PASSWD.contains(&format!("{USER}:x:1000:1000:{USER}:/home/{USER}:/bin/sh")));
        assert!(GROUP.lines().any(|l| l == "baud:x:1000:"));
        // No account has a password to log in by.
        assert!(SHADOW.lines().all(|l| l.split(':').nth(1) == Some("*")));
        assert!(PROFILE.contains("PS1='\\u@\\h:\\w\\$ '"));
        assert!(PROFILE.lines().any(|l| l == "export LANG=C.UTF-8"));
        assert_eq!(
            crate::session::REMOTE_PROMPT,
            format!("{USER}@{HOST}:~$ "),
            "busybox's \\u@\\h:\\w\\$ for the user in its home"
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
            "  User baud",
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
    /// them: each file this module reads installed from the same file,
    /// sshd's directories root's and not writable by others, the shadow
    /// file root's alone, the temporary directory sticky, each package
    /// linked once, sshd the entry point, and the image named [`IMAGE`]
    /// and built for arm64 by the flake's [`FLAKE_OUTPUT`] on this Mac.
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
            "var var/empty".to_string(),
            "install -m 0644 ${./passwd} etc/passwd".to_string(),
            "install -m 0644 ${./group} etc/group".to_string(),
            "install -m 0600 ${./shadow} etc/shadow".to_string(),
            "install -m 0644 ${./profile} etc/profile".to_string(),
            "install -m 0644 ${./sshd_config} etc/ssh/sshd_config".to_string(),
            "install -m 0644 ${./fastfetch.jsonc} home/baud/.config/fastfetch/config.jsonc"
                .to_string(),
            "ln -s ${busybox}/bin bin".to_string(),
            "ln -s ${fastfetchMinimal}/bin/fastfetch usr/bin/fastfetch".to_string(),
            "ln -s ${openssh}/bin/sshd usr/sbin/sshd".to_string(),
            "\"/usr/sbin/sshd\"".to_string(),
        ] {
            assert!(has(NIX, &line), "{line}");
        }
        assert!(NIX.contains("install -d -m 0755 etc etc/ssh home home/baud"));
        assert!(NIX.contains("inherit (linux) busybox openssh fastfetchMinimal;"));
        let wide = NIX.replace("-m 0600 ${./shadow}", "-m 0644 ${./shadow}");
        assert!(
            !has(&wide, "install -m 0600 ${./shadow} etc/shadow"),
            "the control"
        );
        assert!(FLAKE.contains(&format!(
            "aarch64-darwin.{FLAKE_OUTPUT} = import ./nix/demo-container {{"
        )));
        assert!(FLAKE.contains("linux = pkgsFor \"aarch64-linux\";"));
        assert_eq!(
            socket(r#"{"arch":"aarch64","docker_socket":"unix:///x/docker.sock","k":1}"#),
            Some("unix:///x/docker.sock".to_string())
        );
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
    /// names, `context show` answering `now` and the container's removal
    /// failing.
    fn undone(started: bool, was: &str, now: &str) -> Vec<String> {
        let u = Undo {
            colima: Colima {
                started,
                context: Some(was.to_string()),
                host: "unix:///c/docker.sock".to_string(),
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
    /// and then its image removed from colima's docker, the image though
    /// the container's removal failed, colima stopped only if the session
    /// started it, and docker's context put back only if it moved.
    #[test]
    fn the_undo_removes_the_container_and_puts_colima_back() {
        let rm = "DOCKER_HOST=unix:///c/docker.sock docker rm -f baud-demo-7";
        let rmi = "DOCKER_HOST=unix:///c/docker.sock docker image rm -f baud-demo:readme";
        assert_eq!(
            undone(true, "default", "colima"),
            [
                rm,
                rmi,
                "colima stop",
                "docker context show",
                "docker context use default"
            ]
        );
        assert_eq!(
            undone(false, "default", "default"),
            [rm, rmi, "docker context show"]
        );
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
