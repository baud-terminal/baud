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
//! aarch64-linux from cache.nixos.org, so no Linux builder is needed;
//! starship and bash come from the flake's `nixpkgs-demo` input, the
//! revision [`STARSHIP_NIXPKGS`] names, the starship the
//! demo's own shell runs.
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
//! **colima's own profiles are left as they were found**: the session's VM
//! is a profile of its own, [`COLIMA_PROFILE`], made with every CPU of
//! this Mac and docker's context left where it was, and deleted with its
//! data at the end; a profile of that name already there is refused. The
//! container and its image are removed, and the VM deleted, on every way
//! out ([`Running`]'s drop), Ctrl-C among them: its handler only sets
//! [`INTERRUPTED`], and every wait the session makes stops on it and
//! returns, so the drop runs.

use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
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
/// both. Only starship comes from it, so every other program the film
/// shows stays where it was.
pub const STARSHIP_NIXPKGS: &str = "github:NixOS/nixpkgs/13e53762c5afaf384d4614e94527a9ccfafe41d0";

/// **The image**, by the name and tag `nix/demo-container/default.nix`
/// gives it, which the session removes when it ends.
pub const IMAGE: &str = "baud-demo:readme";

/// The repository flake's output that streams the image.
pub const FLAKE_OUTPUT: &str = "demo-container";

/// **The image's stream, by SHA-256**, from the flake's pinned nixpkgs for
/// `aarch64-linux`, the system colima's VM runs on an Apple silicon Mac.
pub const IMAGE_SHA256: &str = "0d1f8760a4a2582c5b1b4ccf54a8b2f64bb6a4176b7b3a4cd128eb3b37496917";

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
/// with `cpus`, colima's defaults for the rest, and docker's context not
/// switched to it, since the session talks to its socket alone.
fn start_args(cpus: usize) -> Vec<String> {
    ["start", "-p", COLIMA_PROFILE, "--cpus"]
        .into_iter()
        .map(str::to_string)
        .chain([cpus.to_string(), "--activate=false".to_string()])
        .collect()
}

/// Whether `colima list --json`, a profile a line, names
/// [`COLIMA_PROFILE`].
fn listed(list: &str) -> bool {
    let name = format!("\"name\":\"{COLIMA_PROFILE}\"");
    list.lines().any(|l| l.contains(&name))
}

/// What docker's context was before the session, and the docker it talks
/// to: the session's VM's socket, empty until it is up.
struct Colima {
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
    /// `run`: the container removed, then its image, once the VM's docker
    /// answers; then the session's VM deleted with its data, however far
    /// its start got; then docker switched back to its context if anything
    /// moved it. Each is tried whatever the one before did.
    fn run(&self, run: &mut Runner) {
        if !self.colima.host.is_empty() {
            let host = [("DOCKER_HOST", self.colima.host.as_str())];
            let _ = run("docker", &["rm", "-f", &self.name], &host);
            let _ = run("docker", &["image", "rm", "-f", &self.image], &host);
        }
        let deleted = run(
            "colima",
            &["delete", "-p", COLIMA_PROFILE, "-f", "--data"],
            &[],
        );
        println!(
            "colima:  the session's VM deleted{}",
            match deleted {
                Ok(_) => String::new(),
                Err(_) => format!(" -- FAILED: run `colima delete -p {COLIMA_PROFILE} -f --data`"),
            }
        );
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

/// **The container up**, its sshd answering on `port`: the session's VM
/// started as [`COLIMA_PROFILE`] with every CPU of this Mac, the image
/// loaded from its checked stream, the host key and the user's public key
/// copied in owned by root, and the container started; then the banner
/// read through colima's port forward. A profile of that name already
/// there is refused, not reused: it is a session's that did not end.
/// Everything it started is undone when the result drops, and on an error
/// before that -- the undo exists before the VM's start, so a start
/// Ctrl-C cut short is deleted too. Ctrl-C ends the wait for the banner
/// ([`INTERRUPTED`]).
pub fn up(image: &Image, keys: &Keys, port: u16) -> Result<Running, String> {
    let tar = image.gnutar.as_path();
    let context = say("docker", &["context", "show"], &[]).ok();
    let list = say("colima", &["list", "--json"], &[])
        .map_err(|e| format!("{e}; the session's container runs in colima"))?;
    if listed(&list) {
        return Err(format!(
            "colima already has a profile {COLIMA_PROFILE}, a session's that did not end; \
             `colima delete -p {COLIMA_PROFILE} -f --data` removes it"
        ));
    }
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut run = Running {
        undo: Undo {
            colima: Colima {
                context,
                host: String::new(),
            },
            name: format!("baud-demo-{}", std::process::id()),
            image: IMAGE.to_string(),
        },
    };
    println!(
        "colima:  starting the session's VM, profile {COLIMA_PROFILE}, with this Mac's {cpus} \
         CPUs; it is deleted at the end"
    );
    let ok = Command::new("colima")
        .args(start_args(cpus))
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("colima: {e}"))?;
    interrupted(&INTERRUPTED)?;
    if !ok.success() {
        return Err("colima did not start the session's VM".to_string());
    }
    run.undo.colima.host = say("colima", &["status", "-p", COLIMA_PROFILE, "--json"], &[])
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
            "ln -s ${fastfetchMinimal}/bin/fastfetch usr/bin/fastfetch".to_string(),
            "ln -s ${btop}/bin/btop usr/bin/btop".to_string(),
            "ln -s ${openssh}/bin/sshd usr/sbin/sshd".to_string(),
            "\"/usr/sbin/sshd\"".to_string(),
        ] {
            assert!(has(NIX, &line), "{line}");
        }
        assert!(NIX.contains("install -d -m 0755 etc etc/ssh home home/jordan"));
        assert!(!NIX.contains("home/baud"), "the user's home is jordan's");
        assert!(NIX.contains("inherit (linux) busybox openssh fastfetchMinimal btop;"));
        assert!(NIX.contains("inherit (prompt) starship bashInteractive;"));
        let wide = NIX.replace("-m 0600 ${./shadow}", "-m 0644 ${./shadow}");
        assert!(
            !has(&wide, "install -m 0600 ${./shadow} etc/shadow"),
            "the control"
        );
        assert!(FLAKE.contains(&format!(
            "aarch64-darwin.{FLAKE_OUTPUT} = import ./nix/demo-container {{"
        )));
        assert!(FLAKE.contains("linux = pkgsFor \"aarch64-linux\";"));
        assert!(FLAKE.contains("prompt = nixpkgs-demo.legacyPackages.aarch64-linux;"));
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
    /// nix. The starship and bash come from the flake's `nixpkgs-demo`,
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
    /// data, and before its docker answered only that; and docker's context
    /// put back only if it moved. No other profile is named: none is
    /// stopped. The VM's start names its own profile, every CPU asked
    /// for, and leaves docker's context; a profile of its name already
    /// listed is found, and the control, another's, is not.
    #[test]
    fn the_undo_removes_the_container_and_deletes_the_vm() {
        let rm = "DOCKER_HOST=unix:///c/docker.sock docker rm -f baud-demo-7";
        let rmi = "DOCKER_HOST=unix:///c/docker.sock docker image rm -f baud-demo:readme";
        let delete = "colima delete -p baud-demo -f --data";
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
                "--activate=false"
            ]
        );
        let list = "{\"name\":\"default\",\"status\":\"Stopped\"}\n";
        assert!(!listed(list), "the control");
        assert!(listed(&format!(
            "{list}{{\"name\":\"baud-demo\",\"status\":\"Running\"}}\n"
        )));
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
