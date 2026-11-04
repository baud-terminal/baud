//! **The session, run**: the programs and faces resolved and checked, the
//! scratch home written, baud built and launched, and every step of
//! [`SESSION`] played into its window.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::container;
use crate::film::{self, Display};
use crate::fonts::{self, PINNED};
use crate::guard::{self, Rect};
use crate::keys::{self, Gaps, RETURN};
use crate::mac;
use crate::names;
use crate::provenance;
use crate::record;
use crate::scratch::{self, Layout};
use crate::session::{self, SESSION, Step};

/// Every program is taken from this nixpkgs revision,
/// `scripts/readme-film.sh`'s.
pub const NIXPKGS: &str = "github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a";

/// The programs the session runs on this Mac: nixpkgs attribute, binary.
/// helix edits baud's config, and OpenSSH's client ssh's into the
/// container (`container.rs`), its manual pages left out; fastfetch and
/// btop run in the container, from its image.
const PROGRAMS: [(&str, &str); 2] = [("helix", "hx"), ("openssh^out", "ssh")];

/// **The headless film's frame period**, in milliseconds: 100 frames a
/// second, the rate the README logo's flashes are counted at
/// (`baud-bench/tests/readme_logo_flashes.rs`), finer than a display's 60.
const CENSUS_PERIOD_MS: u32 = 10;

/// How long a prompt may take to come, and an echo.
const PROMPT_LIMIT: Duration = Duration::from_secs(15);
const ECHO_LIMIT: Duration = Duration::from_secs(3);

/// How long baud's window must keep one frame before the session plays,
/// and how long it may take to.
const SETTLE: f64 = 0.5;
const SETTLE_LIMIT: Duration = Duration::from_secs(5);

/// How long the window stays after `--done` is touched, so a recorder that
/// stops a moment after it still has baud's window in front.
const DONE_TAIL: Duration = Duration::from_millis(1500);

const USAGE: &str = "usage: cargo run --release --example demo -- [--dry] \
                     [--record OUT.gif [--allow-dirty]] [--forbidden LIST --controls FILE] \
                     [--pid-file PATH] [--events PATH] [--done PATH]\n       \
                     cargo run --release --example demo -- --headless DIR \
                     [--forbidden LIST --controls FILE]\n       \
                     cargo run --example demo -- --verify GIF";

struct Args {
    dry: bool,
    headless: Option<PathBuf>,
    forbidden: Option<PathBuf>,
    controls: Option<PathBuf>,
    pid_file: Option<PathBuf>,
    events: Option<PathBuf>,
    done: Option<PathBuf>,
    record: Option<PathBuf>,
    allow_dirty: bool,
    verify: Option<PathBuf>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        dry: false,
        headless: None,
        forbidden: None,
        controls: None,
        pid_file: None,
        events: None,
        done: None,
        record: None,
        allow_dirty: false,
        verify: None,
    };
    while let Some(arg) = it.next() {
        let mut path = || {
            it.next()
                .map(PathBuf::from)
                .ok_or(format!("{arg} takes a path"))
        };
        match arg.as_str() {
            "--dry" => a.dry = true,
            "--headless" => a.headless = Some(path()?),
            "--forbidden" => a.forbidden = Some(path()?),
            "--controls" => a.controls = Some(path()?),
            "--pid-file" => a.pid_file = Some(path()?),
            "--events" => a.events = Some(path()?),
            "--done" => a.done = Some(path()?),
            "--record" => a.record = Some(path()?),
            "--allow-dirty" => a.allow_dirty = true,
            "--verify" => a.verify = Some(path()?),
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("{other} is not an option")),
        }
    }
    if a.allow_dirty && a.record.is_none() {
        return Err("--allow-dirty goes with --record".to_string());
    }
    if a.forbidden.is_some() != a.controls.is_some() {
        return Err("--forbidden and --controls go together".to_string());
    }
    // A film is searched for what must never be on it, and the list of
    // that is kept outside the repository, so a recording names it.
    if a.record.is_some() && a.forbidden.is_none() {
        return Err("--record needs --forbidden LIST --controls FILE".to_string());
    }
    if a.headless.is_some() && (a.dry || a.record.is_some()) {
        return Err("--headless opens no window, so it neither records nor stops dry".to_string());
    }
    let others = a.dry
        || a.headless.is_some()
        || a.forbidden.is_some()
        || a.record.is_some()
        || a.pid_file.is_some()
        || a.events.is_some()
        || a.done.is_some();
    if a.verify.is_some() && others {
        return Err("--verify takes nothing else".to_string());
    }
    Ok(a)
}

pub fn main() -> ExitCode {
    // Ctrl-C only sets a flag the session's waits stop on, so the
    // container is removed and its VM deleted on the way out.
    mac::catch_interrupt();
    let args = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("demo: {e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let ran = run(&args);
    let stopped = container::interrupted(&container::INTERRUPTED);
    // Ctrl-C named before the failure it caused.
    match stopped.and(ran) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("demo: {e}");
            // 128 + SIGINT, as a shell reports a program Ctrl-C ended.
            match container::INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed) {
                true => ExitCode::from(130),
                false => ExitCode::FAILURE,
            }
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let root = provenance::repository();
    if let Some(gif) = &args.verify {
        for l in provenance::verify(gif, &root)? {
            println!("{l}");
        }
        println!(
            "verified: {} is the GIF its provenance file describes",
            gif.display()
        );
        return Ok(());
    }
    // A recording names the commit it filmed, so it is made of a clean
    // tree; this is checked before anything is fetched, built or opened.
    let tree = match &args.record {
        Some(_) => {
            let t = provenance::tree(&root)?;
            provenance::admit(&t, args.allow_dirty)?;
            println!(
                "tree:    {}{}",
                t.commit,
                if t.dirty.is_empty() {
                    ", clean".to_string()
                } else {
                    format!(", {} paths differ, allowed by --allow-dirty", t.dirty.len())
                }
            );
            Some(t)
        }
        None => None,
    };
    let host = std::env::var("TERM_PROGRAM").unwrap_or_else(|_| "this terminal".to_string());
    if !args.dry && !mac::trusted() {
        return Err(format!(
            "the demo types into baud's window, which macOS allows only an application \
             trusted for Accessibility. Allow the one this runs in ({host}) under System \
             Settings > Privacy & Security > Accessibility, then run it again. Nothing \
             was opened and no event was posted."
        ));
    }
    if !args.dry && args.record.is_some() && !mac::may_record() {
        return Err(format!(
            "--record films the screen, which macOS allows only an application trusted for \
             Screen Recording. Allow the one this runs in ({host}) under System Settings > \
             Privacy & Security > Screen & System Audio Recording, then run it again. \
             Nothing was opened."
        ));
    }
    let prep = prepare()?;
    let names = machine_names(&prep.at.root)?;
    let profile = Profile::of_this_example()?;
    let command = profile.command(kept_out(std::env::args().skip(1)));
    let kit = match &args.record {
        Some(out) => {
            let hit = names::named(command.as_bytes(), &names);
            if !hit.is_empty() {
                return Err(format!(
                    "the command names this machine ({hit:?}), and the provenance file \
                     records it: give the GIF's path relative to where this runs"
                ));
            }
            Some(record::kit(out, &prep.at, &root, nix_out)?)
        }
        None => None,
    };
    let built = build_baud(&profile)?;
    let baud = built.bin;
    println!("baud:    {}", baud.display());
    let run = tree.as_ref().map(|t| record::Run {
        root: &root,
        command: command.clone(),
        from: from_dir(&root),
        tree: t,
        tools: &prep.tools,
        pins: &prep.pins,
        names: &names,
    });
    if args.dry {
        if let (Some(k), Some(r)) = (&kit, &run) {
            record::self_check(k, &prep.at, r)?;
        }
        println!(
            "access:  {}",
            if mac::trusted() {
                "this terminal is trusted for Accessibility, so the session can type"
            } else {
                "this terminal is not trusted for Accessibility: the session would stop here"
            }
        );
        if args.record.is_some() {
            println!(
                "screen:  {}",
                if mac::may_record() {
                    "this terminal may film the screen, so --record can film"
                } else {
                    "this terminal may not film the screen: --record would stop here"
                }
            );
        }
        println!(
            "dry:     no window opened, no event posted; the scratch directory is kept at {}",
            prep.at.root.display()
        );
        return Ok(());
    }
    let forbidden = match (&args.forbidden, &args.controls) {
        (Some(list), Some(controls)) => Some(Forbidden {
            python: nix_out("python3^out")?.join("bin/python3"),
            list: list.clone(),
            controls: controls.clone(),
        }),
        _ => None,
    };
    let up = container::up(&prep.image, &prep.keys, prep.port)?;
    if let Some(stills) = &args.headless {
        headless(&profile, &prep, stills, forbidden.as_ref())?;
        drop(up);
        let _ = fs::remove_dir_all(&prep.at.root);
        return Ok(());
    }
    let session = live(args, &prep, &baud, kit.as_ref(), forbidden.as_ref());
    drop(up);
    if let (Some(k), Some(r), Some(s)) = (&kit, &run, session?) {
        record::finish(k, &prep.at, r, &built.target, s)?;
    }
    let _ = fs::remove_dir_all(&prep.at.root);
    Ok(())
}

/// **The README session without a window**: baud's own test of it
/// (`film::session` in `baud-app/src/`), built by the cargo that ran this
/// in its profile, played on the scratch directory against the running
/// container. It types every step into baud's login shell through baud's
/// core, draws through baud's CPU renderer, and fails unless every light
/// but CS lights; the last frame of every hold goes into `stills` as a
/// PNG. Every frame, drawn [`CENSUS_PERIOD_MS`] apart, goes into a
/// lossless film beside them, `session.mkv`, whose trails', fireworks' and
/// clear's shots baud-bench's flash census counts
/// (`baud-bench/tests/session_flashes.rs`). What the programs printed is
/// then checked for a character no face draws, and searched as a film's
/// is.
fn headless(
    p: &Profile,
    prep: &Prep,
    stills: &Path,
    forbidden: Option<&Forbidden>,
) -> Result<(), String> {
    fs::create_dir_all(stills).map_err(|e| format!("{}: {e}", stills.display()))?;
    let ffmpeg = nix_out("ffmpeg-headless.bin")?.join("bin/ffmpeg");
    let film = stills.join("session.mkv");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["test", "-p", "baud", "--bin", "baud", "--manifest-path"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .arg("--target-dir")
        .arg(&p.target)
        .args(p.flags())
        .args(["--", "film::session::the_readme_session_lights_every_light"])
        .args(["--ignored", "--exact", "--nocapture"])
        .env("BAUD_SESSION_ROOT", &prep.at.root)
        .env("BAUD_SESSION_STILLS", stills)
        .env("BAUD_FILM_FFMPEG", &ffmpeg)
        .env("BAUD_SESSION_FILM", &film)
        .env("BAUD_SESSION_PERIOD_MS", CENSUS_PERIOD_MS.to_string())
        .envs(
            ["BAUD_SESSION_POINTS", "BAUD_SESSION_WITHOUT"]
                .into_iter()
                .filter_map(|k| std::env::var_os(k).map(|v| (k, v))),
        )
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        return Err("the session without a window failed".to_string());
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let census = Command::new(cargo)
        .args([
            "test",
            "-p",
            "baud-bench",
            "--test",
            "session_flashes",
            "--manifest-path",
        ])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-bench/Cargo.toml"
        ))
        .arg("--target-dir")
        .arg(&p.target)
        .args(p.flags())
        .args(["--", "the_readme_session_is_below_the_flash_thresholds"])
        .args(["--ignored", "--exact", "--nocapture"])
        .env("BAUD_SESSION_FILM", &film)
        .env("BAUD_FILM_FFMPEG", &ffmpeg)
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !census.success() {
        return Err("the flash census of the session failed".to_string());
    }
    let heard = prep.at.root.join("heard.log");
    check_printed(&prep.faces, &heard)?;
    if let Some(f) = forbidden {
        f.search(&[heard])?;
    }
    println!("stills:  {}", stills.display());
    Ok(())
}

/// **What must never be filmed**, as a list kept outside the repository:
/// `scripts/readme-forbidden.py` searches the session's bytes for it with
/// the pinned Python, once every pattern has been found in its control.
struct Forbidden {
    python: PathBuf,
    list: PathBuf,
    controls: PathBuf,
}

impl Forbidden {
    fn search(&self, files: &[PathBuf]) -> Result<(), String> {
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../scripts/readme-forbidden.py"
        );
        let status = Command::new(&self.python)
            .arg("-I")
            .arg(script)
            .arg(&self.list)
            .arg(&self.controls)
            .args(files)
            .status()
            .map_err(|e| format!("python: {e}"))?;
        match status.code() {
            Some(0) => Ok(()),
            Some(1) => Err("the session's bytes hold what must never be filmed".to_string()),
            _ => Err("the forbidden list could not be searched with".to_string()),
        }
    }
}

/// What the session needs, made and checked: the scratch home, baud's
/// environment, and for the provenance file each nixpkgs attribute's store
/// path and each pinned file's SHA-256.
struct Prep {
    at: Layout,
    env: Vec<(String, String)>,
    tools: Vec<(String, String)>,
    pins: Vec<(String, String)>,
    image: container::Image,
    keys: container::Keys,
    port: u16,
    /// The pinned faces, in the cache, for the check of what was printed.
    faces: Vec<PathBuf>,
}

fn prepare() -> Result<Prep, String> {
    let mut bins = Vec::new();
    let mut tools = Vec::new();
    let mut store = |attr: &str| -> Result<PathBuf, String> {
        let p = nix_out(attr)?;
        tools.push((attr.to_string(), p.to_string_lossy().into_owned()));
        Ok(p)
    };
    for (attr, bin) in PROGRAMS {
        let dir = store(attr)?.join("bin");
        let exe = dir.join(bin);
        if !exe.is_file() {
            return Err(format!("{attr} has no {}", exe.display()));
        }
        println!("{bin:<13}{}", exe.display());
        bins.push(dir.to_string_lossy().into_owned());
    }
    let cache = cache_dir()?;
    let mut faces = Vec::new();
    for p in &PINNED {
        let f = fetch(p, &cache)?;
        println!("{:<13}{} ({})", "font", p.name, p.draws);
        faces.push(f);
    }

    let at = Layout {
        root: fresh_root()?,
    };
    let path = scratch::path(&bins);
    let files: Vec<&str> = PINNED.iter().map(|p| p.file()).collect();
    let mut written = scratch::files(&path, &files);
    for (rel, bytes) in &written {
        let to = at.root.join(rel);
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    for dir in [at.tmp(), at.fonts()] {
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    for (p, from) in PINNED.iter().zip(&faces) {
        fs::copy(from, at.fonts().join(p.file())).map_err(|e| format!("{}: {e}", p.file()))?;
    }
    // The container: its image, built by the repository's flake and its
    // stream checked against its pin, and the session's keys, the scratch
    // home's ssh files among what is checked.
    let ssh = bins
        .iter()
        .find(|b| Path::new(b).join("ssh").is_file())
        .map(|b| Path::new(b).join("ssh-keygen"))
        .ok_or("no ssh-keygen beside ssh")?;
    let port = container::free_port()?;
    let (keys, ssh_files) = container::keys(&at.home(), &at.root.join("container"), &ssh, port)?;
    written.extend(ssh_files);
    let image = container::image(
        &at.root.join("container"),
        &flake_out,
        &nix_out,
        &provenance::sha256,
    )?;
    println!("{:<13}{}", "container", image.archive.display());
    tools.extend(image.tools.iter().cloned());
    let env = scratch::env(&at, &path);
    // baud's environment, a `KEY=value` a line, for the session played
    // without a window (`--headless`).
    let lines: String = env.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    fs::write(at.root.join("env"), lines).map_err(|e| format!("env: {e}"))?;
    let pins: Vec<(String, String)> = PINNED
        .iter()
        .map(|p| (p.file().to_string(), p.sha256.to_string()))
        .collect();

    let config = scratch::config(&files);
    check_fonts(&faces, &config)?;
    check_names(&at, &written, &env)?;
    print_plan();
    Ok(Prep {
        at,
        env,
        tools,
        pins,
        image,
        keys,
        port,
        faces,
    })
}

/// The output path of `attr` at the pinned revision.
pub fn nix_out(attr: &str) -> Result<PathBuf, String> {
    nix_build(&format!("{NIXPKGS}#{attr}"))
}

/// **The repository flake's `output`**, built, and for the provenance
/// file its store path and its packages' (`passthru.packages`).
fn flake_out(output: &str) -> container::Built {
    let at = format!("{}#{output}", provenance::repository().display());
    let path = nix_build(&at)?;
    let out = Command::new("nix")
        .args(["eval", "--json"])
        .arg(format!("{at}.packages"))
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("nix: {e}"))?;
    if !out.status.success() {
        return Err(format!("nix eval {output}.packages failed"));
    }
    let mut tools = vec![(
        format!("flake#{output}"),
        path.to_string_lossy().into_owned(),
    )];
    let json = String::from_utf8_lossy(&out.stdout);
    for pair in json.trim().trim_matches(['{', '}']).split(',') {
        let (k, v) = pair
            .split_once(':')
            .ok_or("no package in the flake's list")?;
        tools.push((
            format!("flake#{output}.{}", k.trim_matches('"')),
            v.trim_matches('"').to_string(),
        ));
    }
    Ok((path, tools))
}

/// `installable`'s one output path, built.
fn nix_build(installable: &str) -> Result<PathBuf, String> {
    let out = Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths"])
        .arg(installable)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("nix: {e}; the demo takes its programs from nix"))?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || path.is_empty() || path.contains('\n') {
        return Err(format!("nix build {installable} failed"));
    }
    Ok(PathBuf::from(path))
}

/// Refuses `file` unless it hashes to `sha256`.
fn pinned(file: &Path, sha256: &str) -> Result<(), String> {
    let out = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(file)
        .output()
        .map_err(|e| format!("shasum: {e}"))?;
    let got = String::from_utf8_lossy(&out.stdout);
    match got.split_whitespace().next() {
        Some(h) if h == sha256 => Ok(()),
        _ => Err(format!("{} is not the pinned file", file.display())),
    }
}

/// The faces' cache, outside the repository: `NOTO_CACHE`, else
/// `$XDG_CACHE_HOME/baud-readme`, else `~/.cache/baud-readme`, the one
/// `scripts/readme-capture.sh` fills.
fn cache_dir() -> Result<PathBuf, String> {
    if let Some(d) = std::env::var_os("NOTO_CACHE") {
        return Ok(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(d).join("baud-readme"));
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".cache/baud-readme"))
        .ok_or_else(|| "no HOME for the font cache; set NOTO_CACHE".to_string())
}

/// The pinned face in the cache, fetched when it is missing, and refused
/// unless it hashes to its pin.
fn fetch(p: &fonts::Pinned, cache: &Path) -> Result<PathBuf, String> {
    let f = cache.join(p.file());
    if !f.exists() {
        fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
        let part = cache.join(format!("{}.part", p.file()));
        let ok = Command::new("/usr/bin/curl")
            .args(["-sSfL", "-o"])
            .arg(&part)
            .arg(p.url)
            .status()
            .map_err(|e| format!("curl: {e}"))?;
        if !ok.success() {
            return Err(format!("could not fetch {}", p.url));
        }
        fs::rename(&part, &f).map_err(|e| format!("{}: {e}", f.display()))?;
    }
    pinned(&f, p.sha256)?;
    Ok(f)
}

/// A new directory in the system's temporary one.
fn fresh_root() -> Result<PathBuf, String> {
    let tmp = std::env::temp_dir();
    for n in 0..100 {
        let dir = tmp.join(format!("baud-demo-{}-{n}", std::process::id()));
        if fs::create_dir(&dir).is_ok() {
            return Ok(dir);
        }
    }
    Err(format!("no new directory in {}", tmp.display()))
}

/// **The faces baud draws the session with**, in the order of the stack
/// baud builds from the scratch config: its text face, its icon face, the
/// four pinned faces, STIX Two Math.
fn stack(faces: &[Vec<u8>]) -> Vec<(&'static str, &[u8])> {
    let mut stack: Vec<(&str, &[u8])> = vec![
        ("Proto Hebrew Mono C", fonts::REGULAR),
        ("Symbols Nerd Font Mono", fonts::SYMBOLS),
    ];
    stack.extend(
        PINNED
            .iter()
            .zip(faces)
            .map(|(p, b)| (p.name, b.as_slice())),
    );
    stack.push(("STIX Two Math", fonts::STIX));
    stack
}

/// The pinned faces' bytes.
fn read_faces(faces: &[PathBuf]) -> Result<Vec<Vec<u8>>, String> {
    faces
        .iter()
        .map(|f| fs::read(f).map_err(|e| format!("{}: {e}", f.display())))
        .collect()
}

/// **Which characters of `text` no face of the stack draws**, each listed
/// with the face that draws it, under `what`; an error naming them when
/// there are any, as baud would ask the system for those.
fn covered(stack: &[(&str, &[u8])], what: &str, text: &str) -> Result<(), String> {
    let c = fonts::coverage(stack, text)?;
    println!("fonts:   the {} characters of {what}:", c.checked);
    for (name, chars) in &c.drawn {
        let sample: String = chars.iter().take(24).collect();
        println!("         {:<24}{:>4}  {sample}", name, chars.len());
    }
    if !c.gaps.is_empty() {
        let g: String = c.gaps.iter().collect();
        return Err(format!(
            "no pinned face draws {g}, in {what}: the system would be asked for them"
        ));
    }
    Ok(())
}

/// **No character the session shows from what it controls needs a system
/// face**: `hello.txt`, the config helix shows, both prompts and the
/// fireworks' request, through the stack baud builds from the scratch
/// config and baud-render's table. Then the control: without the CJK face,
/// 终 is a gap, so the check can fail. What the programs print is checked
/// once they have printed it ([`check_printed`]).
fn check_fonts(faces: &[PathBuf], config: &str) -> Result<(), String> {
    let bytes = read_faces(faces)?;
    let stack = stack(&bytes);
    let text = format!(
        "{}{config}{}{}{}",
        session::HELLO,
        session::PROMPT_SHOWN,
        session::REMOTE_PROMPT,
        session::FIREWORKS
    );
    covered(
        &stack,
        "hello.txt, the config, both prompts and the fireworks' request",
        &text,
    )?;
    let without: Vec<(&str, &[u8])> = stack
        .iter()
        .copied()
        .filter(|(n, _)| *n != "Noto Sans CJK SC")
        .collect();
    let control = fonts::coverage(&without, &text)?;
    if !control.gaps.contains(&'终') {
        return Err("the font check cannot fail: 终 drew without the CJK face".to_string());
    }
    println!(
        "         none from the system; without Noto Sans CJK SC, {} are, 终 among them",
        control.gaps.len()
    );
    Ok(())
}

/// **No character the programs printed needs a system face**: every
/// character of `log` -- what baud read from the shell and the programs
/// it ran, helix, fastfetch and btop among them, and what was typed --
/// through the same stack and table. A sequence's own characters are
/// ASCII, which baud's text face draws, so the log is read whole rather
/// than parsed: a title, which only the title bar shows, is checked too.
fn check_printed(faces: &[PathBuf], log: &Path) -> Result<(), String> {
    let bytes = read_faces(faces)?;
    let printed = fs::read(log).map_err(|e| format!("{}: {e}", log.display()))?;
    covered(
        &stack(&bytes),
        "what the programs printed",
        &String::from_utf8_lossy(&printed),
    )?;
    println!("         none from the system");
    Ok(())
}

/// **Nothing written names this machine**: each text file of the scratch
/// directory, the repository's address in the README aside, and baud's
/// environment, the scratch directory aside (`names::env_named`).
fn check_names(
    at: &Layout,
    written: &[(String, Vec<u8>)],
    env: &[(String, String)],
) -> Result<(), String> {
    let n = machine_names(&at.root)?;
    for name in &n {
        if !names::named(name.as_bytes(), &n).contains(name) {
            return Err(format!("the names check cannot find {name}"));
        }
    }
    let public = names::public_path(env!("CARGO_PKG_REPOSITORY"));
    for (rel, bytes) in written {
        let hit = names::named_outside(bytes, &n, &[&public]);
        if !hit.is_empty() {
            return Err(format!("{rel} names this machine: {hit:?}"));
        }
    }
    let root = at.root.to_string_lossy();
    let hit = names::env_named(env, &root, &n);
    if !hit.is_empty() {
        return Err(format!("baud's environment names this machine: {hit:?}"));
    }
    println!(
        "names:   the {} files written and baud's {} variables name none of this machine's {} \
         names but the scratch directory; each name, written, is found",
        written.len(),
        env.len(),
        n.len()
    );
    Ok(())
}

/// This machine's names: [`names::machine_names`] over this process's
/// environment and temporary directory, the host name and the computer's
/// two names; and the scratch root, which the pty log must not name.
fn machine_names(root: &Path) -> Result<Vec<String>, String> {
    let env: Vec<(String, String)> = std::env::vars().collect();
    let say = |cmd: &str, args: &[&str]| {
        Command::new(cmd)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let hosts = [
        say("/bin/hostname", &[]),
        say("/usr/sbin/scutil", &["--get", "ComputerName"]),
        say("/usr/sbin/scutil", &["--get", "LocalHostName"]),
    ];
    if hosts[0].is_empty() {
        return Err("hostname said nothing, so the names check would miss the host".to_string());
    }
    let temp = std::env::temp_dir().to_string_lossy().into_owned();
    let mut n = names::machine_names(&env, &temp, &hosts);
    n.push(root.to_string_lossy().into_owned());
    Ok(n)
}

fn print_plan() {
    let d = session::duration(SESSION);
    println!(
        "plan:    {} steps, {} prompts, at least {:.1} s plus each program's start, \
         {} at {} pt",
        SESSION.len(),
        session::prompts(SESSION),
        d.as_secs_f64(),
        scratch::GRID,
        scratch::POINTS
    );
    let mut prompt = 0;
    for (i, step) in SESSION.iter().enumerate() {
        if *step == Step::Prompt {
            prompt += 1;
        }
        println!(
            "         {:>2}  {}",
            i + 1,
            session::describe(*step, prompt)
        );
    }
}

/// **Where cargo put this example**: `<target>/[<triple>/]<profile>/examples/demo`,
/// read from its own path.
struct Profile {
    dir: PathBuf,
    name: String,
    target: PathBuf,
    triple: Option<String>,
}

impl Profile {
    fn of_this_example() -> Result<Profile, String> {
        let exe = std::env::current_exe().map_err(|e| format!("where this example is: {e}"))?;
        let dir = exe
            .parent()
            .and_then(Path::parent)
            .ok_or("the example is not in a cargo target directory")?;
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("the example is not in a cargo target directory")?;
        let mut target = dir.parent().ok_or("no target directory")?;
        let triple = target
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|t| t.ends_with("-apple-darwin"))
            .map(str::to_string);
        if triple.is_some() {
            target = target.parent().ok_or("no target directory")?;
        }
        Ok(Profile {
            dir: dir.to_path_buf(),
            name: name.to_string(),
            target: target.to_path_buf(),
            triple,
        })
    }

    /// cargo's flags for this profile and target.
    fn flags(&self) -> Vec<String> {
        let mut f = match self.name.as_str() {
            "debug" => Vec::new(),
            "release" => vec!["--release".to_string()],
            other => vec!["--profile".to_string(), other.to_string()],
        };
        if let Some(t) = &self.triple {
            f.extend(["--target".to_string(), t.clone()]);
        }
        f
    }

    /// **The command that ran this**, as it would be typed: `cargo run`, the
    /// profile's flags, `--example demo --` and the example's arguments,
    /// each quoted for a shell when it needs it.
    fn command(&self, args: impl Iterator<Item = String>) -> String {
        let mut words = vec!["cargo".to_string(), "run".to_string()];
        words.extend(self.flags());
        words.extend(["--example", "demo", "--"].map(str::to_string));
        words.extend(args.map(|a| shell_word(&a)));
        words.join(" ")
    }
}

/// A word as a shell reads it back: bare when it is plain, else in single
/// quotes.
fn shell_word(a: &str) -> String {
    let plain = !a.is_empty()
        && a.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./=:,+-@%".contains(c));
    if plain {
        a.to_string()
    } else {
        format!("'{}'", a.replace('\'', "'\\''"))
    }
}

/// The directory this runs from, from the repository's root: `.` there.
fn from_dir(root: &Path) -> String {
    let here = std::env::current_dir().and_then(fs::canonicalize);
    let root = fs::canonicalize(root);
    match (here, root) {
        (Ok(h), Ok(r)) if h == r => ".".to_string(),
        (Ok(h), Ok(r)) => h
            .strip_prefix(&r)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "outside the repository".to_string()),
        _ => "unknown".to_string(),
    }
}

/// The `baud` built, and the target directory it is in.
struct Built {
    bin: PathBuf,
    target: PathBuf,
}

/// **The current `baud`**, built by the cargo that ran this example
/// (`CARGO`, which `cargo run` sets) in the example's own profile and
/// target directory, read from where the example's binary is:
/// `<target>/<profile>/examples/demo`. `cargo run` lets go of its build
/// lock before it starts the example, so the build can take it.
fn build_baud(p: &Profile) -> Result<Built, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(["build", "-p", "baud", "--bin", "baud", "--manifest-path"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .arg("--target-dir")
        .arg(&p.target)
        .args(p.flags());
    let status = cmd.status().map_err(|e| format!("cargo: {e}"))?;
    let bin = p.dir.join("baud");
    if !status.success() || !bin.is_file() {
        return Err(format!(
            "cargo build -p baud did not make {}",
            bin.display()
        ));
    }
    Ok(Built {
        bin,
        target: p.target.clone(),
    })
}

/// The pty log baud writes (`BAUD_PTYLOG`), read as it grows.
struct PtyLog {
    path: PathBuf,
    bytes: Vec<u8>,
    marks: usize,
    counted: usize,
}

impl PtyLog {
    fn refresh(&mut self) {
        if let Ok(mut f) = File::open(&self.path) {
            let _ = f.seek(SeekFrom::Start(self.bytes.len() as u64));
            let _ = f.read_to_end(&mut self.bytes);
        }
        let from = self.counted.saturating_sub(session::PROMPT_MARK.len() - 1);
        self.marks += session::marks(&self.bytes[from..]);
        self.counted = self.bytes.len();
    }

    fn wait(
        &mut self,
        limit: Duration,
        what: &str,
        done: impl Fn(&PtyLog) -> bool,
    ) -> Result<(), String> {
        let t = Instant::now();
        loop {
            container::interrupted(&container::INTERRUPTED)?;
            self.refresh();
            if done(self) {
                return Ok(());
            }
            if t.elapsed() > limit {
                return Err(format!("{what} never came"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// **One timestamped line a step**, on stdout, in `--events` and in the
/// provenance file: seconds since baud's launch, negative for what came
/// before it -- the recorder's start. A line written before the launch
/// waits for it. Shared with the thread that reads the recorder.
#[derive(Clone)]
pub struct Events(Arc<Mutex<Log>>);

pub struct Log {
    launch: Option<Instant>,
    pending: Vec<(Instant, String)>,
    file: Option<File>,
    lines: Vec<String>,
}

impl Log {
    fn write(&mut self, t: f64, what: &str) {
        let l = format!("{t:.3} {what}");
        println!("         {l}");
        if let Some(f) = &mut self.file {
            let _ = writeln!(f, "{l}");
        }
        self.lines.push(l);
    }
}

/// `t` in seconds after `from`, negative before it.
fn since(t: Instant, from: Instant) -> f64 {
    if t >= from {
        t.duration_since(from).as_secs_f64()
    } else {
        -from.duration_since(t).as_secs_f64()
    }
}

impl Events {
    fn new(file: Option<File>) -> Events {
        Events(Arc::new(Mutex::new(Log {
            launch: None,
            pending: Vec::new(),
            file,
            lines: Vec::new(),
        })))
    }

    pub fn line(&self, what: &str) {
        let now = Instant::now();
        let Ok(mut log) = self.0.lock() else { return };
        match log.launch {
            Some(l) => log.write(since(now, l), what),
            None => log.pending.push((now, what.to_string())),
        }
    }

    /// baud was launched `at`: the lines before it are written now.
    fn launched(&self, at: Instant) {
        let Ok(mut log) = self.0.lock() else { return };
        log.launch = Some(at);
        for (t, what) in std::mem::take(&mut log.pending) {
            log.write(since(t, at), &what);
        }
    }

    fn lines(&self) -> Vec<String> {
        self.0.lock().map(|l| l.lines.clone()).unwrap_or_default()
    }
}

/// A pid written whole: to a file beside it, then renamed over it, so a
/// reader never sees part of it.
fn write_pid(p: &Path, pid: i32) -> Result<(), String> {
    let mut part = p.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    fs::write(&part, format!("{pid}\n"))
        .and_then(|()| fs::rename(&part, p))
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// **The session played**, and with `--record` filmed: the recorder starts
/// and must be filming before baud is launched. Returns what the GIF
/// needs when it filmed.
fn live(
    args: &Args,
    prep: &Prep,
    baud: &Path,
    kit: Option<&record::Kit>,
    forbidden: Option<&Forbidden>,
) -> Result<Option<record::Session>, String> {
    let at = &prep.at;
    let log = File::create(at.baud_log()).map_err(|e| format!("baud's log: {e}"))?;
    let err = log.try_clone().map_err(|e| format!("baud's log: {e}"))?;
    let events = match &args.events {
        Some(p) => Some(File::create(p).map_err(|e| format!("{}: {e}", p.display()))?),
        None => None,
    };
    let ev = Events::new(events);
    let mut filming = match kit {
        Some(k) => Some(record::start(k, at, &ev)?),
        None => None,
    };
    let launch = Instant::now();
    let spawned = Command::new(baud)
        .env_clear()
        .envs(prep.env.iter().map(|(k, v)| (k, v)))
        .current_dir(at.work())
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err)
        .spawn();
    ev.launched(launch);
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            if let Some(f) = &mut filming {
                f.stop();
            }
            return Err(format!("{}: {e}", baud.display()));
        }
    };
    let Ok(pid) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        if let Some(f) = &mut filming {
            f.stop();
        }
        return Err("a pid past i32".to_string());
    };
    ev.line(&format!("baud pid {pid}"));
    let mut pid_files: Vec<PathBuf> = args.pid_file.iter().cloned().collect();
    let mut dones: Vec<PathBuf> = args.done.iter().cloned().collect();
    if filming.is_some() {
        pid_files.push(at.root.join("baud.pid"));
        dones.push(at.root.join("done"));
    }
    let fixed = match &filming {
        Some(f) => Fixed::Yes(f.display),
        None => Fixed::No,
    };
    let played = pid_files
        .iter()
        .try_for_each(|p| write_pid(p, pid))
        .and_then(|()| play(&ev, pid, at, fixed));
    match &played {
        Ok(_) => ev.line("done"),
        Err(e) => ev.line(&format!("DIE {e}")),
    }
    if !dones.is_empty() {
        for d in &dones {
            let _ = File::create(d);
        }
        if played.is_ok() {
            std::thread::sleep(DONE_TAIL);
        }
    }
    let last = guard::frame(pid, &mac::windows(true));
    close(&mut child, pid);
    let display = filming.as_ref().map(|f| f.display);
    let said = filming.map(|f| f.finish(Duration::from_secs(60)));
    let first = played?;
    leaks(at)?;
    check_printed(&prep.faces, &at.pty_log())?;
    if let Some(f) = forbidden {
        f.search(&[at.pty_log()])?;
    }
    let (Some(said), Some(display)) = (said, display) else {
        return Ok(None);
    };
    let said = said?;
    if last != Some(first) {
        return Err(format!(
            "baud's window moved or changed size while it was filmed ({first:?}, then {last:?}), \
             so no one crop holds it"
        ));
    }
    Ok(Some(record::Session {
        events: ev.lines(),
        said,
        display,
        frame: first,
    }))
}

/// The window closed: baud sent `SIGTERM`, by its pid, and `SIGKILL` if it
/// is still running three seconds later.
fn close(child: &mut Child, pid: i32) {
    let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(3) {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Whether baud's window must stay where it was first seen: when it is
/// filmed, on the display the recorder films, as the film is cropped to
/// it.
#[derive(Clone, Copy)]
enum Fixed {
    Yes(Display),
    No,
}

/// **Every step played**; returns baud's window as it was first seen,
/// which the film is cropped to. The window must have held still there for
/// [`SETTLE`] before the first step (`guard::settle`), and when it is
/// `Fixed`, lie wholly on the display filmed; it is looked at again at
/// every prompt, and a window that has moved or changed size stops the
/// session there rather than at its end.
fn play(ev: &Events, pid: i32, at: &Layout, fixed: Fixed) -> Result<Rect, String> {
    let begun = Instant::now();
    let mut seen: Vec<(f64, Option<Rect>)> = Vec::new();
    let look = |seen: &mut Vec<(f64, Option<Rect>)>| {
        seen.push((
            begun.elapsed().as_secs_f64(),
            guard::frame(pid, &mac::windows(true)),
        ));
    };
    let mut tries = 0;
    while mac::front_owner() != Some(pid) {
        look(&mut seen);
        mac::raise(pid);
        std::thread::sleep(Duration::from_millis(50));
        tries += 1;
        if tries > 100 {
            return Err("baud never came frontmost".to_string());
        }
    }
    ev.line("frontmost");
    let settling = Instant::now();
    let frame = loop {
        look(&mut seen);
        match guard::settle(&seen, SETTLE) {
            guard::Settle::Still(f) => break f,
            guard::Settle::Moved { first, now } => {
                return Err(format!(
                    "baud's window moved after it was first seen ({first:?}, then {now:?}), \
                     and the session crops to where it was first seen, so no one crop holds it"
                ));
            }
            guard::Settle::Waiting if settling.elapsed() > SETTLE_LIMIT => {
                return Err(format!(
                    "baud's window did not hold still for {SETTLE} s within {} s",
                    SETTLE_LIMIT.as_secs()
                ));
            }
            guard::Settle::Waiting => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    ev.line("still");
    if let Fixed::Yes(display) = fixed {
        film::crop(&frame, &display)?;
    }
    let poster = mac::Poster::new(pid)?;
    let seed = pid.unsigned_abs();
    let hold = Duration::from_secs_f64(session::key_hold_ms() / 1000.0);
    let type_keys = |keys: &[keys::Key], gaps: &mut Gaps| -> Result<(), String> {
        for (i, k) in keys.iter().enumerate() {
            poster.key(*k, hold)?;
            if i + 1 < keys.len() {
                let g = gaps.next_ms(session::KEY_GAP_MS) - session::key_hold_ms();
                std::thread::sleep(Duration::from_secs_f64(g.max(0.0) / 1000.0));
            }
        }
        Ok(())
    };
    let mut log = PtyLog {
        path: at.pty_log(),
        bytes: Vec::new(),
        marks: 0,
        counted: 0,
    };
    let mut prompt = 0;
    for &step in SESSION {
        container::interrupted(&container::INTERRUPTED)?;
        match step {
            Step::Prompt => {
                prompt += 1;
                log.wait(PROMPT_LIMIT, &format!("prompt {prompt}"), |l| {
                    l.marks >= prompt
                })?;
                ev.line(&session::event(step, prompt));
                if prompt == 1 {
                    check_config(at)?;
                }
                if matches!(fixed, Fixed::Yes(_)) {
                    let now = guard::frame(pid, &mac::windows(true));
                    if now != Some(frame) {
                        return Err(format!(
                            "baud's window moved or changed size by prompt {prompt} \
                             ({frame:?}, then {now:?}), so no one crop holds it"
                        ));
                    }
                }
            }
            Step::Run(t) => {
                ev.line(&session::event(step, prompt));
                let ks = keys::keys_for(t).map_err(|c| format!("no key types {c:?}"))?;
                let mut gaps = Gaps::new(seed, ks.len() + 1);
                log.refresh();
                let from = log.bytes.len();
                type_keys(&ks, &mut gaps)?;
                let what = format!("the echo of `{t}` (are the keys arriving?)");
                log.wait(ECHO_LIMIT, &what, |l| session::echoed(&l.bytes, from, t))?;
                let g = gaps.next_ms(session::KEY_GAP_MS) - session::key_hold_ms();
                std::thread::sleep(Duration::from_secs_f64(g.max(0.0) / 1000.0));
                type_keys(&[RETURN], &mut gaps)?;
            }
            Step::Keys(_) | Step::Line(_) | Step::Escape => {
                ev.line(&session::event(step, prompt));
                let ks = keys::presses(step).map_err(|c| format!("no key types {c:?}"))?;
                type_keys(&ks, &mut Gaps::new(seed, ks.len()))?;
            }
            Step::Hold(ms) => {
                ev.line(&session::event(step, prompt));
                std::thread::sleep(Duration::from_millis(u64::from(ms)));
            }
            Step::Wheel { turns, lines } => {
                let c = guard::centre(pid, &mac::windows(false))
                    .ok_or("baud has no window on screen")?;
                poster.wheel(c, turns, lines)?;
                ev.line(&session::event(step, prompt));
            }
        }
    }
    Ok(frame)
}

/// **baud read the scratch config and said nothing against it**: its log
/// names that file, and no other line of `config:`.
fn check_config(at: &Layout) -> Result<(), String> {
    let log = fs::read_to_string(at.baud_log()).unwrap_or_default();
    let want = format!("config: {}", at.config().display());
    if !log.lines().any(|l| l == want) {
        return Err("baud did not read its config".to_string());
    }
    let said: Vec<&str> = log
        .lines()
        .filter(|l| l.starts_with("config: ") && *l != want)
        .collect();
    if !said.is_empty() {
        return Err(format!("baud complained about its config: {said:?}"));
    }
    Ok(())
}

/// **Nothing baud was sent names this machine**, the scratch directory
/// included: what the programs printed is on film. The repository's
/// address, which the README prints, aside.
fn leaks(at: &Layout) -> Result<(), String> {
    let bytes = fs::read(at.pty_log()).map_err(|e| format!("the pty log: {e}"))?;
    let n = machine_names(&at.root)?;
    let public = names::public_path(env!("CARGO_PKG_REPOSITORY"));
    let hit = names::named_outside(&bytes, &n, &[&public]);
    if !hit.is_empty() {
        return Err(format!("the screen was sent this machine's names: {hit:?}"));
    }
    println!("names:   nothing the programs printed names this machine");
    Ok(())
}

/// **The arguments as the provenance file records them**: the paths of the
/// forbidden list and its controls are kept outside the repository and
/// name where, so each is recorded as the word the usage gives it.
fn kept_out(args: impl Iterator<Item = String>) -> impl Iterator<Item = String> {
    let mut hide = None;
    args.map(move |a| match hide.take() {
        Some(word) => word,
        None => {
            hide = match a.as_str() {
                "--forbidden" => Some("LIST".to_string()),
                "--controls" => Some("FILE".to_string()),
                _ => None,
            };
            a
        }
    })
}

#[cfg(test)]
mod tests {
    /// **The recorded command names neither the forbidden list nor its
    /// controls**, whose paths are this machine's, and keeps every other
    /// argument as typed. The control: an argument after another flag is
    /// kept.
    #[test]
    fn the_recorded_command_keeps_the_list_out() {
        let args = [
            "--record",
            "assets/readme/hero.gif",
            "--forbidden",
            "lists/forbidden.txt",
            "--controls",
            "lists/controls.txt",
        ];
        let got: Vec<String> = super::kept_out(args.iter().map(|a| a.to_string())).collect();
        assert_eq!(
            got,
            [
                "--record",
                "assets/readme/hero.gif",
                "--forbidden",
                "LIST",
                "--controls",
                "FILE"
            ]
        );
    }
}
