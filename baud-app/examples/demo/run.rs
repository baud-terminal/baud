//! **The session, run**: the programs and faces resolved and checked, the
//! scratch home written, baud built and launched, and every step of the
//! take played into its window: the tour, [`SESSION`], or with `--trailer`
//! the trailer, [`session::trailer`].

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use xtask::forbidden::{self, Verdict};

use crate::container;
use crate::film::{self, Display};
use crate::fonts::{self, PINNED};
use crate::guard::{self, Rect};
use crate::keys::{self, Gaps, RETURN};
use crate::mac;
use crate::names;
use crate::provenance;
use crate::readme_gif;
use crate::record;
use crate::scratch::{self, Layout};
use crate::session::{self, SESSION, Step};

/// Every program is taken from this nixpkgs revision,
/// `scripts/readme-film.sh`'s.
pub const NIXPKGS: &str = "github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a";

/// The oldest starship the session runs: the release that fixed both
/// advisories.
const STARSHIP_AT_LEAST: (u32, u32) = (1, 27);

/// **Whether `starship --version`'s output names a starship at least
/// [`STARSHIP_AT_LEAST`]**, by its first line, `starship 1.27.0`.
fn starship_new_enough(version: &str) -> bool {
    let Some(v) = version
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("starship "))
    else {
        return false;
    };
    let mut parts = v.split('.').map(|n| n.parse::<u32>());
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= STARSHIP_AT_LEAST,
        _ => false,
    }
}

/// The programs the session runs on this Mac: nixpkgs attribute, binary.
/// helix edits baud's config, OpenSSH's client ssh's into the container
/// (`container.rs`), its manual pages left out, kitten draws the pictures,
/// yt-dlp finds the stream of the tour the trailer plays in mpv
/// ([`MPV_OUTPUT`]), yazi lists the repository's copy and opens its README
/// in helix (`scratch::YAZI_TOML`), and Chawan, `cha`, shows a web page
/// (`scratch::CHAWAN_TOML`); fastfetch, btop and the container's helix run
/// in the container, from its image.
///
/// Each names its `out` output: `nix build` prints a path for every output
/// it builds, and [`nix_out`] takes one. A package that grows a second
/// installed output -- yt-dlp's manual, `man`, at [`NIXPKGS`] -- otherwise
/// fails the session's first step.
const PROGRAMS: [(&str, &str); 6] = [
    ("helix^out", "hx"),
    ("openssh^out", "ssh"),
    ("kitty^out", "kitten"),
    ("yt-dlp^out", "yt-dlp"),
    ("yazi^out", "yazi"),
    ("chawan^out", "cha"),
];

/// **The repository flake's mpv** (`nix/demo-mpv/`): nixpkgs' mpv at
/// [`NIXPKGS`], built from mpv's master at [`MPV_REV`], which holds
/// 12d630d819, the status line locked against the VOs' frames, and is
/// 8e566099cb, whose vo_kitty uninit ends its `a=d` with `ST`. 0.41.0 has
/// neither: its status line cut frames, and its quit left a control string
/// open (`tests/recordings/kitty/mpv-logo-40x6.raw`).
pub const MPV_OUTPUT: &str = "demo-mpv";
pub const MPV_REV: &str = "8e566099cbbbfbd9177c457ed26ac51223ba7303";

/// **The headless film's frame period**, in milliseconds: 100 frames a
/// second, the rate the README logo's flashes are counted at
/// (`baud-bench/tests/readme_logo_flashes.rs`), finer than a display's 60.
const CENSUS_PERIOD_MS: u32 = 10;

/// How long a prompt may take to come, and an echo.
const PROMPT_LIMIT: Duration = Duration::from_secs(15);
const ECHO_LIMIT: Duration = Duration::from_secs(3);

/// **How long a program may take to show its first picture**: the trailer's
/// mpv waits on yt-dlp finding the tour's stream over the network.
const PICTURE_LIMIT: Duration = Duration::from_secs(60);

/// How long baud's window must keep one frame before the session plays,
/// and how long it may take to.
const SETTLE: f64 = 0.5;
const SETTLE_LIMIT: Duration = Duration::from_secs(5);

/// How long the window stays after `--done` is touched, so a recorder that
/// stops a moment after it still has baud's window in front.
const DONE_TAIL: Duration = Duration::from_millis(1500);

const USAGE: &str = "usage: cargo run --release --example demo -- [--dry] \
                     [--record OUT.mp4 [--allow-dirty]] [--forbidden LIST --controls FILE] \
                     [--pid-file PATH] [--events PATH] [--done PATH]\n       \
                     cargo run --release --example demo -- --trailer --tour URL --start T --end T \
                     [--dry] \
                     [--record OUT.gif [--allow-dirty]] [--forbidden LIST --controls FILE]\n       \
                     cargo run --release --example demo -- --headless DIR [--allow-dirty] \
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
    /// Take 2, the trailer, the tour's address on YouTube it plays, and
    /// the stretch of it, from `start` to `end`.
    trailer: bool,
    tour: Option<String>,
    start: Option<String>,
    end: Option<String>,
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
        trailer: false,
        tour: None,
        start: None,
        end: None,
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
            "--trailer" => a.trailer = true,
            "--tour" => a.tour = Some(it.next().ok_or("--tour takes the tour's address")?),
            "--start" => a.start = Some(it.next().ok_or("--start takes a time in the tour")?),
            "--end" => a.end = Some(it.next().ok_or("--end takes a time in the tour")?),
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("{other} is not an option")),
        }
    }
    if a.allow_dirty && a.record.is_none() && a.headless.is_none() {
        return Err("--allow-dirty goes with --record or --headless".to_string());
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
    // Take 2 plays a stretch of the tour from YouTube, so it is refused
    // without the tour's address and the stretch, and they without take 2.
    if a.trailer {
        session::trailer(a.tour.as_deref(), a.start.as_deref(), a.end.as_deref())?;
        if a.headless.is_some() {
            return Err("--headless plays the tour; the trailer needs the window".to_string());
        }
    } else if a.tour.is_some() || a.start.is_some() || a.end.is_some() {
        return Err(
            "--tour, --start and --end are what the trailer plays: they go with --trailer"
                .to_string(),
        );
    }
    // The tour is filmed as an MP4 for YouTube, with its timeline for the
    // sound, and the trailer as the README's GIF.
    if let Some(out) = &a.record {
        let name = out.file_name().and_then(|n| n.to_str()).unwrap_or("");
        match (a.trailer, record::Form::of(name)) {
            (true, Some(record::Form::Gif)) | (false, Some(record::Form::Mp4)) => {}
            (true, _) => {
                return Err("--trailer records the README's GIF: --record OUT.gif".to_string());
            }
            (false, _) => {
                return Err("the tour records an MP4 for YouTube: --record OUT.mp4".to_string());
            }
        }
    }
    let others = a.dry
        || a.headless.is_some()
        || a.forbidden.is_some()
        || a.record.is_some()
        || a.pid_file.is_some()
        || a.events.is_some()
        || a.done.is_some()
        || a.trailer;
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
    // The headless run makes the README's GIF, whose strip names the build
    // on every frame, so it is held to the same.
    let tree = match args.record.is_some() || args.headless.is_some() {
        true => {
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
        false => None,
    };
    // The headless run posts no event and films no screen, so it returns
    // before the window's session asks macOS for either.
    if let Some(stills) = &args.headless {
        let tree = tree
            .as_ref()
            .ok_or("the headless run reads its tree first")?;
        headless_take(args, &root, tree, stills)?;
        return Ok(());
    }
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
    // `--dry` only reports, so it is not refused for the state of the
    // session.
    if !args.dry {
        clear_to_open()?;
        guard::space(mac::main_space()).map_err(|r| format!("{}. Nothing was opened.", r.0))?;
    }
    let take = match args.trailer {
        true => Take::Trailer,
        false => Take::Tour,
    };
    let steps: Vec<Step> = match take {
        Take::Trailer => session::trailer(
            args.tour.as_deref(),
            args.start.as_deref(),
            args.end.as_deref(),
        )?,
        Take::Tour => SESSION.to_vec(),
    };
    let mut prep = prepare(&steps, take)?;
    let names = machine_names(&prep.at.root)?;
    let profile = Profile::of_this_example()?;
    if take == Take::Tour {
        let root = provenance::repository();
        let pins = pictures(&profile, &prep.at, &root)?;
        prep.pins.extend(pins);
    }
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
    // An MP4's take journals what baud showed, which only a baud built with
    // the `take` feature can; every other run builds baud as it ships.
    let journal = kit.as_ref().is_some_and(|k| k.form == record::Form::Mp4);
    let built = build_baud(&profile, journal)?;
    let baud = built.bin;
    match journal {
        true => println!("baud:    {}, with --features take", baud.display()),
        false => println!("baud:    {}", baud.display()),
    }
    let opener = build_opener(&profile, &prep.at)?;
    println!("opener:  {}", opener.display());
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
        println!("session: {}", guard::describe(&mac::console()));
        let space = mac::main_space();
        println!(
            "space:   {}",
            match (space, guard::space(space)) {
                (_, Err(why)) => format!("{}: the session would stop here", why.0),
                (Some(_), Ok(())) => "the main display is on a normal desktop, by \
                                      com.apple.spaces, which may be stale"
                    .to_string(),
                (None, Ok(())) => "com.apple.spaces says nothing of the main display, which \
                                   stops nothing"
                    .to_string(),
            }
        );
        println!(
            "dry:     no window opened, no event posted; the scratch directory is kept at {}",
            prep.at.root.display()
        );
        return Ok(());
    }
    let forbidden = match (&args.forbidden, &args.controls) {
        (Some(list), Some(controls)) => Some(Forbidden {
            list: list.clone(),
            controls: controls.clone(),
        }),
        _ => None,
    };
    // Building and fetching can take minutes, so this is asked again.
    clear_to_open()?;
    // The trailer plays the tour from YouTube and goes nowhere over ssh, so
    // it starts no VM.
    let up = match take {
        Take::Tour => Some(container::up(
            &prep.image,
            &prep.keys,
            &prep.home,
            prep.port,
        )?),
        Take::Trailer => None,
    };
    let session = live(args, &prep, &baud, kit.as_ref(), forbidden.as_ref(), &steps);
    drop(up);
    if let (Some(k), Some(r), Some(s)) = (&kit, &run, session?) {
        // The trailer's GIF is censused for flashes where it was made,
        // before it is copied anywhere.
        let gate = |gif: &Path| match take {
            Take::Trailer => flash_gate(&profile, gif),
            Take::Tour => Ok(()),
        };
        record::finish(k, &prep.at, r, &built.target, s, &gate)?;
        keep_full_screen(&prep.at.root, &built.target, &k.gif)?;
    }
    let _ = fs::remove_dir_all(&prep.at.root);
    Ok(())
}

/// **The headless take**: the tour prepared, baud and the opener built, the
/// container started, and the session filmed with no window
/// ([`headless`]), asking macOS for no Accessibility and no Screen
/// Recording, as it posts no event and films no screen.
fn headless_take(
    args: &Args,
    root: &Path,
    tree: &provenance::Tree,
    stills: &Path,
) -> Result<(), String> {
    let steps = SESSION.to_vec();
    let prep = prepare(&steps, Take::Tour)?;
    let names = machine_names(&prep.at.root)?;
    let profile = Profile::of_this_example()?;
    let command = profile.command(kept_out(std::env::args().skip(1)));
    let built = build_baud(&profile, false)?;
    println!("baud:    {}", built.bin.display());
    let opener = build_opener(&profile, &prep.at)?;
    println!("opener:  {}", opener.display());
    let forbidden = args
        .forbidden
        .clone()
        .zip(args.controls.clone())
        .map(|(list, controls)| Forbidden { list, controls });
    let up = container::up(&prep.image, &prep.keys, &prep.home, prep.port)?;
    let made = Made {
        root,
        tree,
        command: &command,
        names: &names,
    };
    headless(&profile, &prep, stills, forbidden.as_ref(), &made)?;
    drop(up);
    let _ = fs::remove_dir_all(&prep.at.root);
    Ok(())
}

/// **The session may take a window**: [`guard::console`] of the session as
/// it is now, in the words of what was not done.
fn clear_to_open() -> Result<(), String> {
    guard::console(&mac::console())
        .map_err(|r| format!("{}; nothing was opened and no event was posted", r.0))
}

/// **The full screen's reading kept beside the raw film**, in cargo's
/// target directory as `NAME.full-screen.json`, not beside the GIF: a file
/// left untracked in the repository would make the next filming run's tree
/// dirty. None is kept when the reading failed.
fn keep_full_screen(root: &Path, target: &Path, gif: &str) -> Result<(), String> {
    let read = root.join("full-screen").join(FULL_SCREEN_OUT);
    if !read.is_file() {
        return Ok(());
    }
    let dir = target.join("demo");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let to = dir.join(full_screen_name(gif));
    fs::copy(&read, &to).map_err(|e| format!("{}: {e}", to.display()))?;
    println!(
        "full:    the window through full screen and back, read into {}",
        to.display()
    );
    Ok(())
}

/// `NAME.full-screen.json` for the GIF `NAME.gif` or the MP4 `NAME.mp4`.
fn full_screen_name(gif: &str) -> String {
    let stem = gif
        .strip_suffix(".gif")
        .or_else(|| gif.strip_suffix(".mp4"));
    format!("{}.full-screen.json", stem.unwrap_or(gif))
}

/// **The README session without a window**: baud's own test of it
/// (`film::session` in `baud-app/src/`), built by the cargo that ran this
/// in its profile, played on the scratch directory against the running
/// container. It types every step into baud's login shell through baud's
/// core, draws through baud's CPU renderer, and fails unless every light
/// lights; the last frame of every hold goes into `stills` as a
/// PNG. Every frame, drawn [`CENSUS_PERIOD_MS`] apart, goes into a
/// lossless film beside them, `session.mkv`, whose trails', fireworks' and
/// clear's shots baud-bench's flash census counts
/// (`baud-bench/tests/session_flashes.rs`). The tour's pinned `cha`, whose
/// window the session cannot open, must then draw a page's picture by
/// kitty's protocol on a pty whose answers are baud's
/// (`programs::chawan_draws_a_pages_picture_by_kittys_protocol`). What the
/// programs printed is
/// then checked for a character no face draws, and searched as a film's
/// is, as the session showed it ([`shown_log`]), which is kept beside the
/// stills as `heard.shown.log`. Every byte the session read from the shell,
/// ssh's session among them, for a replay through baud's core, is kept raw
/// in cargo's target directory ([`keep_raw`]), never beside the stills.
///
/// The session's test is handed this machine's names
/// ([`machine_names`], the git e-mail addresses with them) and the
/// container's user and host, and fails if any frame's text shows one of
/// the first or no frame the second; `--forbidden`'s list searches every
/// frame's text too. Then the README's GIF is cut from the film
/// ([`readme_gif`]): `stills/session.gif`, encoded by the pinned ffmpeg and
/// gifski, decoded and held to the cut, censused as the trailer is
/// ([`flash_gate`]), and its provenance file written beside it, names
/// checked, and read back as `--verify` reads it.
fn headless(
    p: &Profile,
    prep: &Prep,
    stills: &Path,
    forbidden: Option<&Forbidden>,
    made: &Made,
) -> Result<(), String> {
    fs::create_dir_all(stills).map_err(|e| format!("{}: {e}", stills.display()))?;
    let ffmpeg = nix_out("ffmpeg-headless.bin")?.join("bin/ffmpeg");
    let film = stills.join("session.mkv");
    let mut shown_names = made.names.to_vec();
    shown_names.extend(readme_gif::git_emails(made.root));
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
        // The link layer's dumps, beside the stills (`film/tap.rs`): they
        // write the session's home as `~` and its root as `<root>`.
        .env("BAUD_SESSION_LINKS", stills.join("links"))
        // The account the shell runs as, given to the test and written
        // nowhere (`mac::with_account`).
        .env("BAUD_SESSION_ACCOUNT", mac::account()?)
        // What no frame may show, found now and written nowhere, and what
        // the container's card must.
        .env("BAUD_SESSION_NAMES", shown_names.join("\n"))
        .env(
            "BAUD_SESSION_SHOWN",
            format!("{}@{}", container::USER, container::HOST),
        )
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
    // Chawan's pictures, which the session cannot show: its link window is
    // taken out. The pinned `cha` on the session's `PATH`, on a pty whose
    // answers are baud's.
    let path = prep
        .env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map_or("", |(_, v)| v.as_str());
    let cha = std::env::split_paths(path)
        .map(|d| d.join("cha"))
        .find(|p| p.is_file())
        .ok_or("cha is not on the session's PATH")?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let chawan = Command::new(cargo)
        .args(["test", "-p", "baud", "--bin", "baud", "--manifest-path"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .arg("--target-dir")
        .arg(&p.target)
        .args(p.flags())
        .args([
            "--",
            "programs::chawan_draws_a_pages_picture_by_kittys_protocol",
        ])
        .args(["--ignored", "--exact", "--nocapture"])
        .env("BAUD_CHA", &cha)
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !chawan.success() {
        return Err("Chawan drew no picture by kitty's protocol".to_string());
    }
    let heard = prep.at.root.join("heard.log");
    check_printed(&prep.faces, &heard)?;
    // Searched as the session showed it, each folder report as the title
    // shows its folder (`shown_log`); that copy beside the stills, the raw
    // bytes in cargo's target directory, never beside them.
    let shown = shown_log(&heard, &prep.at.home(), &stills.join(HEARD_SHOWN))?;
    leaks(&shown, &prep.at.root)?;
    // Each frame's text as drawn, which the session's test wrote beside
    // the film.
    let text = stills.join("session.mkv.text");
    if let Some(f) = forbidden {
        f.search(&[shown.clone(), text])?;
    }
    let raw = keep_raw(&heard, &p.target)?;
    println!(
        "heard:   {} (as shown), {} (raw)",
        shown.display(),
        raw.display()
    );
    println!("stills:  {}", stills.display());
    cut_the_gif(p, prep, stills, forbidden.is_some(), made)?;
    Ok(())
}

/// **What the README's GIF's provenance file is made from**, of the run
/// that made it: the repository, its tree, the command as typed and this
/// machine's names, which the file must not hold.
struct Made<'a> {
    root: &'a Path,
    tree: &'a provenance::Tree,
    command: &'a str,
    names: &'a [String],
}

/// **The README's GIF, from the session's film** (`demo/readme_gif.rs`):
/// [`readme_gif::SCENES`] cut by the tour's marks, encoded, decoded and
/// held to the cut, censused for flashes as the trailer is, and its
/// provenance file written beside it, which must name nothing of this
/// machine, then read back as `--verify` reads it.
fn cut_the_gif(
    p: &Profile,
    prep: &Prep,
    stills: &Path,
    searched: bool,
    made: &Made,
) -> Result<(), String> {
    let film_at = stills.join("session.mkv");
    let cut_at = stills.join("session.mkv.cut");
    let cut_text = fs::read_to_string(&cut_at).map_err(|e| format!("{}: {e}", cut_at.display()))?;
    let c = readme_gif::cut(&cut_text, &readme_gif::SCENES)?;
    let t = readme_gif::tools(nix_out)?;
    let gif = stills.join(readme_gif::GIF);
    let m = readme_gif::encode(&t, &film_at, &c, &prep.at.root, &gif)?;
    let gif = fs::canonicalize(&gif).map_err(|e| format!("{}: {e}", gif.display()))?;
    flash_gate(p, &gif)?;
    let d = m.decoded;
    println!(
        "gif:     {}: scenes {:?}, frames {:?} of the film, {:.2} s; {} frames sampled, {} \
         shown, {:.2} s, {}x{}",
        gif.display(),
        readme_gif::SCENES,
        c.ranges,
        c.seconds(),
        m.pngs,
        d.frames,
        d.millis as f64 / 1000.0,
        d.width,
        d.height
    );
    let mut checks = vec![
        format!(
            "{}: every light, the trail plasma through the README's scene and each jump's \
             plasma, no frame's text showing this machine's names, the container's user and \
             host shown",
            readme_gif::SESSION_TEST
        ),
        "the session film's flash census, WCAG 2.3.1 (baud-bench/tests/session_flashes.rs)"
            .to_string(),
        "this GIF's flash census, WCAG 2.3.1 (baud-bench/tests/trailer_flashes.rs)".to_string(),
        format!(
            "decoded by baud's own decoder: {} frames, {:.3} s, {}x{}, as long as the cut",
            d.frames,
            d.millis as f64 / 1000.0,
            d.width,
            d.height
        ),
    ];
    if searched {
        checks.push(
            "the forbidden list's search of every frame's text and the session's bytes, \
             each pattern found in its control first; the list is kept outside the repository"
                .to_string(),
        );
    }
    let dir = stills.to_string_lossy();
    let at_dir = |s: &str| s.replace(&*dir, "<DIR>");
    let frames_args: Vec<String> = std::iter::once("ffmpeg".to_string())
        .chain(readme_gif::frames_args(
            "<DIR>/session.mkv",
            &c,
            &format!("{}/gif-frames", record::SCRATCH),
        ))
        .collect();
    let gifski_args: Vec<String> = std::iter::once("gifski".to_string())
        .chain(film::gifski_args(&format!("<DIR>/{}", readme_gif::GIF)))
        .chain(std::iter::once(format!(
            "{}/gif-frames/*.png",
            record::SCRATCH
        )))
        .collect();
    let facts = readme_gif::Facts {
        command: at_dir(made.command),
        from: from_dir(made.root),
        tree: made.tree,
        gif_sha256: provenance::sha256(&gif)?,
        gif_bytes: fs::metadata(&gif)
            .map_err(|e| format!("{}: {e}", gif.display()))?
            .len(),
        made: &m,
        film_sha256: provenance::sha256(&film_at)?,
        film_frames: c.film,
        cut: &c,
        frames_args,
        gifski_args,
        tools: &t,
        checks,
    };
    let header = readme_gif::header();
    let header: Vec<&str> = header.iter().map(String::as_str).collect();
    let text = readme_gif::document(&facts).text(&header);
    let public = names::public_path(env!("CARGO_PKG_REPOSITORY"));
    let hit = names::named_outside(text.as_bytes(), made.names, &[&public]);
    if !hit.is_empty() {
        return Err(format!(
            "the GIF's provenance file would name this machine: {} names; nothing is written",
            hit.len()
        ));
    }
    let prov = provenance::path_of(&gif);
    fs::write(&prov, text).map_err(|e| format!("{}: {e}", prov.display()))?;
    for l in provenance::verify(&gif, made.root)? {
        println!("{l}");
    }
    println!(
        "verified: {} is the GIF its provenance file describes",
        gif.display()
    );
    Ok(())
}

/// The name of what a headless session showed, beside its stills.
const HEARD_SHOWN: &str = "heard.shown.log";

/// The name of the raw heard bytes, in cargo's target directory.
const HEARD_RAW: &str = "heard.raw.log";

/// **What a session showed, of the bytes at `raw`**, written to `to`: each
/// shell folder report (`OSC 7`) as the window's title shows its folder,
/// relative to `home`, the session's, and everything else as it came
/// (`undrawn.rs`). The core draws no report, and the title never shows the
/// host one names, so this is what the searches for this machine's names
/// and for what must never be filmed read.
fn shown_log(raw: &Path, home: &Path, to: &Path) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = fs::read(raw).map_err(|e| format!("{}: {e}", raw.display()))?;
    let home = fs::canonicalize(home).map_err(|e| format!("{}: {e}", home.display()))?;
    let shown = crate::undrawn::shown_bytes(&bytes, Some(home.as_os_str().as_bytes()));
    fs::write(to, shown).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(to.to_path_buf())
}

/// **The raw heard bytes kept in cargo's target directory**, as
/// `demo/heard.raw.log`: never beside the stills, which may be anywhere,
/// and never untracked in the repository, as `keep_full_screen` keeps its
/// reading. The scratch root's own copy goes with the root on success.
fn keep_raw(heard: &Path, target: &Path) -> Result<PathBuf, String> {
    let dir = target.join("demo");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let to = dir.join(HEARD_RAW);
    fs::copy(heard, &to).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(to)
}

/// **What must never be filmed**, as a list kept outside the repository:
/// `xtask::forbidden` searches the session's bytes for it, once every
/// pattern has been found in its control.
struct Forbidden {
    list: PathBuf,
    controls: PathBuf,
}

impl Forbidden {
    fn search(&self, files: &[PathBuf]) -> Result<(), String> {
        let verdict = forbidden::search(
            &self.list,
            &self.controls,
            files,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr().lock(),
        );
        match verdict {
            Ok(Verdict::Clean) => Ok(()),
            Ok(Verdict::Hits) => {
                Err("the session's bytes hold what must never be filmed".to_string())
            }
            Ok(Verdict::Refused(_)) | Err(_) => {
                Err("the forbidden list could not be searched with".to_string())
            }
        }
    }
}

/// **The trailer GIF's flash gate** (WCAG 2.3.1): baud-bench's census of
/// the GIF where the take made it, in the scratch directory, before
/// anything is copied out (`record::keep_gif`), run by the cargo that ran
/// this in its profile (`baud-bench/tests/trailer_flashes.rs`). The path is
/// absolute, as the test runs in `baud-bench/`, and the gate passes only
/// when that one test ran and passed.
fn flash_gate(p: &Profile, gif: &Path) -> Result<(), String> {
    let mut cmd = gate_command(p, gif)?;
    let out = cmd.output().map_err(|e| format!("cargo: {e}"))?;
    let said = String::from_utf8_lossy(&out.stdout);
    print!("{said}");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    if out.status.success() && ran_one(&said) {
        println!("flashes: {} passes WCAG 2.3.1's census", gif.display());
        return Ok(());
    }
    Err(format!(
        "{} fails WCAG 2.3.1's flash census, or the census did not run: nothing is kept",
        gif.display()
    ))
}

/// **The gate's command**, refused for a path that is not absolute: the
/// census runs with its working directory in `baud-bench/`, where a
/// relative path names another file.
fn gate_command(p: &Profile, gif: &Path) -> Result<Command, String> {
    if !gif.is_absolute() {
        return Err(format!(
            "the flash census reads {} from baud-bench/, so it is given an absolute path",
            gif.display()
        ));
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(["test", "-p", "baud-bench", "--test", "trailer_flashes"])
        .arg("--manifest-path")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-bench/Cargo.toml"
        ))
        .arg("--target-dir")
        .arg(&p.target)
        .args(p.flags())
        .args(["--", "the_trailer_is_below_the_flash_thresholds"])
        .args(["--ignored", "--exact", "--nocapture"])
        .env("BAUD_TRAILER_GIF", gif);
    Ok(cmd)
}

/// **Whether libtest's output says one test ran and passed**: under
/// `--exact`, a test renamed away matches nothing and passes with none run.
fn ran_one(stdout: &str) -> bool {
    stdout
        .lines()
        .any(|l| l.trim_start().starts_with("test result: ok. 1 passed;"))
}

/// **Which take**: take 1, the tour, or take 2, the trailer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Take {
    Tour,
    Trailer,
}

/// **`git archive`'s arguments for the repository's copy**: the take's
/// commit, `HEAD`, never the working tree, and [`scratch::REPO_COPY`]'s
/// paths alone.
fn archive_args() -> Vec<&'static str> {
    let mut a = vec!["archive", "--format=tar", "HEAD", "--"];
    a.extend(scratch::REPO_COPY);
    a
}

/// **A file of the take's commit**, `git show HEAD:path`.
fn at_head(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", &format!("HEAD:{path}")])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    match out.status.success() {
        true => Ok(out.stdout),
        false => Err(format!("git show HEAD:{path} failed")),
    }
}

/// **The repository's copy, at the take's commit**: [`archive_args`]'s
/// tar of the repository at `root`, unpacked into `dest`. Returns the
/// files the archive held, by their paths, folders left out.
fn copy_repo(root: &Path, dest: &Path) -> Result<Vec<String>, String> {
    fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let tar = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(archive_args())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git archive: {e}"))?;
    if !tar.status.success() {
        return Err(format!(
            "git archive of the repository's copy failed: {}",
            String::from_utf8_lossy(&tar.stderr).trim()
        ));
    }
    let run_tar = |args: &[&str]| -> Result<Vec<u8>, String> {
        let mut t = Command::new("/usr/bin/tar")
            .args(args)
            .current_dir(dest)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("tar: {e}"))?;
        if let Some(mut i) = t.stdin.take() {
            std::io::Write::write_all(&mut i, &tar.stdout).map_err(|e| format!("tar: {e}"))?;
        }
        let out = t.wait_with_output().map_err(|e| format!("tar: {e}"))?;
        match out.status.success() {
            true => Ok(out.stdout),
            false => Err(format!("tar {} failed", args.join(" "))),
        }
    };
    let listed = run_tar(&["-tf", "-"])?;
    run_tar(&["-xf", "-"])?;
    Ok(String::from_utf8_lossy(&listed)
        .lines()
        .filter(|l| !l.ends_with('/'))
        .map(str::to_string)
        .collect())
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
    /// The files the container's home is given (`container::home_files`).
    home: Vec<PathBuf>,
    port: u16,
    /// The pinned faces, in the cache, for the check of what was printed.
    faces: Vec<PathBuf>,
}

fn prepare(steps: &[Step], take: Take) -> Result<Prep, String> {
    if take == Take::Tour {
        session::readme_scene(scratch::README)?;
        // The storyline's must-shows, each found in the steps.
        let shown = session::shows(steps);
        let missing: Vec<session::Show> = session::Show::ALL
            .into_iter()
            .filter(|s| !shown.contains(s))
            .collect();
        if !missing.is_empty() {
            return Err(format!("the tour no longer shows {missing:?}"));
        }
    }
    let mut bins = Vec::new();
    let mut tools = Vec::new();
    // starship, from its own revision, refused unless it is outside both
    // advisories.
    let starship_at = format!("{}#starship", container::STARSHIP_NIXPKGS);
    let starship = nix_build(&starship_at)?.join("bin");
    let version = Command::new(starship.join("starship"))
        .arg("--version")
        .env_clear()
        .output()
        .map_err(|e| format!("starship: {e}"))?;
    let version = String::from_utf8_lossy(&version.stdout).into_owned();
    if !starship_new_enough(&version) {
        return Err(format!(
            "{starship_at} is {:?}; the session runs starship 1.27.0 or later",
            version.lines().next().unwrap_or("")
        ));
    }
    println!("{:<13}{}", "starship", starship.join("starship").display());
    tools.push((
        starship_at,
        starship
            .parent()
            .unwrap_or(&starship)
            .to_string_lossy()
            .into_owned(),
    ));
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
    let (mpv, mpv_tools) = flake_out(MPV_OUTPUT)?;
    let at = format!("{}#{MPV_OUTPUT}.rev", provenance::repository().display());
    let rev = Command::new("nix")
        .args(["eval", "--raw"])
        .arg(&at)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("nix: {e}"))?;
    if !rev.status.success() || rev.stdout != MPV_REV.as_bytes() {
        return Err(format!(
            "{at} is {:?}, not {MPV_REV}",
            String::from_utf8_lossy(&rev.stdout)
        ));
    }
    let exe = mpv.join("bin").join("mpv");
    if !exe.is_file() {
        return Err(format!("{MPV_OUTPUT} has no {}", exe.display()));
    }
    println!("{:<13}{}", "mpv", exe.display());
    bins.push(mpv.join("bin").to_string_lossy().into_owned());
    tools.extend(mpv_tools);
    bins.push(starship.to_string_lossy().into_owned());
    let cache = cache_dir()?;
    let mut faces = Vec::new();
    for p in &PINNED {
        let f = fetch(p, &cache)?;
        println!("{:<13}{} ({})", "font", p.name, p.draws);
        faces.push(f);
    }

    let at = Layout {
        root: scratch::fresh_root(&std::env::temp_dir())?,
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
    // The repository's copy yazi opens, at the take's commit: the
    // repository's own published files, so not among what the names check
    // reads, which `THIRD_PARTY.md`'s `tinted-theming/home/` would trip.
    let repo = provenance::repository();
    let copied = copy_repo(&repo, &at.work().join(scratch::REPO_AT))?;
    println!(
        "{:<13}{} files at HEAD into ~/baud/{}",
        "repository",
        copied.len(),
        scratch::REPO_AT
    );
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
    let readme = at_head(&repo, "README.md")?;
    let (home, home_files) = container::home(&at.root.join("container"), &readme)?;
    written.extend(home_files);
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
    check_fonts(&faces, &config, steps)?;
    check_names(&at, &written, &env)?;
    print_plan(steps);
    Ok(Prep {
        at,
        env,
        tools,
        pins,
        image,
        keys,
        home,
        port,
        faces,
    })
}

/// **The three pictures the tour draws in its working directory**:
/// `README.md` at the take's commit, which scene 5 opens in helix; the
/// photo, [`scratch::PHOTO_AT`], fetched by its pin
/// (`assets/demo/photo.toml`), refused unless it hashes to it, and scaled
/// to its width there by the pinned ffmpeg; [`session::LOGO_MP4`], scene
/// 10's film, the dark half of `baud-pictures logo --mp4 --out DIR` -- made
/// into the scratch directory, never into the tree, by `tools/baud-pictures`
/// in `tools/shell.nix`, which pins the C it links -- and
/// [`session::UFO_LOGO`], scene 9's second picture, copied from
/// [`session::UFO_LOGO_FROM`]. Returns each file's name and SHA-256, for
/// the provenance file.
fn pictures(p: &Profile, at: &Layout, root: &Path) -> Result<Vec<(String, String)>, String> {
    let work = at.work();
    let readme = work.join("README.md");
    fs::write(&readme, at_head(root, "README.md")?)
        .map_err(|e| format!("{}: {e}", readme.display()))?;
    let (pin, width) = scratch::photo()?;
    let photo = fetch(&pin, &cache_dir()?)?;
    let t = readme_gif::tools(nix_out)?;
    let shown = at.root.join(scratch::PHOTO_AT);
    let scaled = Command::new(&t.ffmpeg)
        .args(["-v", "error", "-y", "-i"])
        .arg(&photo)
        .args(["-vf", &format!("scale={width}:-2")])
        .args(["-q:v", "2"])
        .arg(&shown)
        .status()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !scaled.success() {
        return Err(format!("ffmpeg could not scale {}", photo.display()));
    }
    println!("{:<13}{} ({})", "photo", pin.name, pin.draws);
    let mut pins = vec![(pin.file().to_string(), pin.sha256.to_string())];
    let out = at.root.join("logo");
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let quote = |s: &std::ffi::OsStr| format!("'{}'", s.to_string_lossy().replace('\'', "'\\''"));
    let line = format!(
        "{} run --release --manifest-path {} --target-dir {} --bin baud-pictures -- logo --mp4 \
         --out {}",
        quote(&cargo),
        quote(root.join("tools/Cargo.toml").as_os_str()),
        quote(p.target.join("tools").as_os_str()),
        quote(out.as_os_str()),
    );
    let made = Command::new("nix-shell")
        .arg(root.join("tools/shell.nix"))
        .args(["--run", &line])
        .status()
        .map_err(|e| format!("nix-shell: {e}"))?;
    if !made.success() {
        return Err("baud-pictures logo --mp4 could not make the logo's film".to_string());
    }
    for (from, to) in [
        (out.join("logo-dark.mp4"), session::LOGO_MP4),
        (root.join(session::UFO_LOGO_FROM), session::UFO_LOGO),
    ] {
        let dest = work.join(to);
        fs::copy(&from, &dest).map_err(|e| format!("{}: {e}", from.display()))?;
        pins.push((to.to_string(), provenance::sha256(&dest)?));
        println!("{:<13}{to}, from {}", "picture", from.display());
    }
    Ok(pins)
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

/// **The faces baud draws the session with**, in the order of the stack
/// baud builds from the scratch config: its text face, its icon face, the
/// four pinned faces, STIX Two Math.
fn stack(faces: &[Vec<u8>]) -> Vec<(&'static str, &[u8])> {
    let mut stack: Vec<(&str, &[u8])> = vec![
        ("baud Mono", fonts::REGULAR),
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
/// face**: `hello.txt`, the config and the README helix shows,
/// both prompts and everything the take types -- the narration, the
/// commands, the fireworks' request -- through the stack baud builds from
/// the scratch config and baud-render's table. Then the control: without
/// the CJK face, 波 is a gap, so the check can fail. What the programs
/// print is checked once they have printed it ([`check_printed`]).
fn check_fonts(faces: &[PathBuf], config: &str, steps: &[Step]) -> Result<(), String> {
    let bytes = read_faces(faces)?;
    let stack = stack(&bytes);
    let typed: String = steps
        .iter()
        .filter_map(|s| match s {
            Step::Run(t) | Step::Keys(t) | Step::Line(t) => Some(*t),
            _ => None,
        })
        .collect();
    let text = format!(
        "{}{config}{}{}{}{typed}",
        session::HELLO,
        scratch::README,
        session::PROMPT_SHOWN,
        session::REMOTE_PROMPT,
    );
    covered(
        &stack,
        "hello.txt, the config, the README, both prompts and what the take types",
        &text,
    )?;
    let without: Vec<(&str, &[u8])> = stack
        .iter()
        .copied()
        .filter(|(n, _)| *n != "Noto Sans CJK SC")
        .collect();
    let control = fonts::coverage(&without, &text)?;
    if !control.gaps.contains(&'波') {
        return Err("the font check cannot fail: 波 drew without the CJK face".to_string());
    }
    println!(
        "         none from the system; without Noto Sans CJK SC, {} are, 波 among them",
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
/// environment and temporary directory, the host name, the computer's
/// two names and the chip's model; and the temporary directory and the scratch root in every
/// spelling ([`names::scratch_names`]), which the pty log must not name.
fn machine_names(root: &Path) -> Result<Vec<String>, String> {
    let env: Vec<(String, String)> = std::env::vars().collect();
    let say = |cmd: &str, args: &[&str]| {
        Command::new(cmd)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    // The chip's model too: the container's btop is told a name of its own
    // for it (`nix/demo-container/btop.conf`), and fastfetch's card names
    // the VM's processor, which must not be this Mac's model.
    let hosts = [
        say("/bin/hostname", &[]),
        say("/usr/sbin/scutil", &["--get", "ComputerName"]),
        say("/usr/sbin/scutil", &["--get", "LocalHostName"]),
        say("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]),
    ];
    if hosts[0].is_empty() {
        return Err("hostname said nothing, so the names check would miss the host".to_string());
    }
    let temp = std::env::temp_dir();
    let mut n = names::machine_names(&env, &temp.to_string_lossy(), &hosts);
    n.extend(names::scratch_names(&temp, root));
    n.sort();
    n.dedup();
    Ok(n)
}

fn print_plan(steps: &[Step]) {
    let d = session::duration(steps);
    println!(
        "plan:    {} steps, {} prompts, at least {:.1} s plus each program's start, \
         {} at {} pt",
        steps.len(),
        session::prompts(steps),
        d.as_secs_f64(),
        scratch::GRID,
        scratch::POINTS
    );
    for (n, says, at) in session::scenes(steps) {
        println!(
            "         scene {n:>2} from step {:>3}: {}",
            at.start + 1,
            says.unwrap_or("(no line)")
        );
    }
    let mut prompt = 0;
    for (i, step) in steps.iter().enumerate() {
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
/// lock before it starts the example, so the build can take it. With
/// `journal`, for an MP4's take, built with the `take` feature, which
/// changes only whether baud can journal (`baud-app/src/take.rs`).
fn build_baud(p: &Profile, journal: bool) -> Result<Built, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(["build", "-p", "baud", "--bin", "baud", "--manifest-path"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .arg("--target-dir")
        .arg(&p.target)
        .args(p.flags());
    if journal {
        cmd.args(["--features", "take"]);
    }
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

/// **`demo-open`, built and put on the shell's `PATH`**: the
/// `demo-open` example (`baud-app/examples/demo-open.rs`), built as baud
/// is, copied to `~/bin/demo-open` in the scratch home ([`scratch::BIN`]),
/// which the `.zshrc` puts first on the `PATH`.
fn build_opener(p: &Profile, at: &Layout) -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "build",
            "-p",
            "baud",
            "--example",
            "demo-open",
            "--manifest-path",
        ])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .arg("--target-dir")
        .arg(&p.target)
        .args(p.flags())
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    let built = p.dir.join("examples/demo-open");
    if !status.success() || !built.is_file() {
        return Err(format!(
            "cargo build --example demo-open did not make {}",
            built.display()
        ));
    }
    let bin = at.home().join(scratch::BIN);
    fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let to = bin.join("demo-open");
    fs::copy(&built, &to).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(to)
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

/// **The variable that arms baud's full-screen reading**, and the files it
/// is started by and writes, as `baud-app/src/full_screen_check.rs` names
/// them.
const FULL_SCREEN_ENV: &str = "BAUD_FULL_SCREEN_CHECK";
const FULL_SCREEN_GO: &str = "full-screen.go";
const FULL_SCREEN_OUT: &str = "full-screen.json";

/// **The variable that starts baud's take journal**, as
/// `baud-app/src/take.rs` names it, and the journal's file in the scratch
/// root: what an MP4's timeline is made from.
const TAKE_ENV: &str = "BAUD_TAKE";
const TAKE_JOURNAL: &str = "take.tsv";

/// How long the reading may take: about two seconds in full screen, and
/// the two transitions.
const FULL_SCREEN_LIMIT: Duration = Duration::from_secs(20);

/// **baud's window read through a full screen and back, once the film is
/// whole**: the file that starts the reading made in `dir`, which baud
/// watches, and the reading baud writes there waited for, whole. baud
/// enters and leaves full screen by the window's own `toggleFullScreen:`;
/// nothing here moves the pointer or posts an event. A reading that never
/// comes is said and costs nothing else: the film is already whole.
fn full_screen(dir: &Path) -> Result<(), String> {
    let go = dir.join(FULL_SCREEN_GO);
    fs::write(&go, b"").map_err(|e| format!("{}: {e}", go.display()))?;
    let out = dir.join(FULL_SCREEN_OUT);
    let t = Instant::now();
    loop {
        container::interrupted(&container::INTERRUPTED)?;
        // baud writes the file in one call, and a reading cut short reads
        // as no closing brace.
        let whole = fs::read_to_string(&out).is_ok_and(|s| s.trim_end().ends_with('}'));
        if whole {
            return Ok(());
        }
        if t.elapsed() > FULL_SCREEN_LIMIT {
            let _ = fs::remove_file(&out);
            return Err("baud never wrote its full screen's reading".to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
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
/// needs when it filmed. A filmed session, once the recorder has the
/// film's last frame, reads baud's window through one full screen and
/// back ([`full_screen`]) before the window closes, and the reading is
/// kept beside the raw film ([`keep_full_screen`]).
fn live(
    args: &Args,
    prep: &Prep,
    baud: &Path,
    kit: Option<&record::Kit>,
    forbidden: Option<&Forbidden>,
    steps: &[Step],
) -> Result<Option<record::Session>, String> {
    // The VM can take minutes to start, so this is asked once more.
    clear_to_open()?;
    // The pasteboard, kept before anything is filmed and put back on every
    // way out of this function (`mac::Kept`).
    let board = mac::Kept::of(mac::General);
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
    // The filming run alone arms baud's full-screen reading, in a
    // directory of the scratch root.
    let check = at.root.join("full-screen");
    // An MP4's take also journals what baud showed, for its timeline.
    let journal = at.root.join(TAKE_JOURNAL);
    let mut armed: Vec<(&str, &Path)> = match &filming {
        Some(_) => {
            fs::create_dir_all(&check).map_err(|e| format!("{}: {e}", check.display()))?;
            vec![(FULL_SCREEN_ENV, check.as_path())]
        }
        None => Vec::new(),
    };
    if kit.is_some_and(|k| k.form == record::Form::Mp4) {
        armed.push((TAKE_ENV, journal.as_path()));
    }
    // Who the shell runs as, as launchd tells a Dock-started baud, given
    // here and written nowhere (`mac::with_account`).
    let account = mac::account()?;
    let launch = Instant::now();
    let spawned = Command::new(baud)
        .env_clear()
        .envs(mac::with_account(&prep.env, &account))
        .envs(armed.iter().copied())
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
    let mut scenes = Vec::new();
    let played = pid_files
        .iter()
        .try_for_each(|p| write_pid(p, pid))
        .and_then(|()| play(&ev, pid, at, fixed, &board, &mut scenes, steps));
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
    let display = filming.as_ref().map(|f| f.display);
    let started = filming.as_ref().and_then(record::Filming::started);
    let said = filming.map(|f| f.finish(Duration::from_secs(60), mac::console));
    // The film whole, its window read through a full screen and back.
    if let (Some(_), Some(Ok(_)), Ok(_)) = (kit, &said, &played) {
        if let Err(e) = full_screen(&check) {
            println!("full:    no reading of the full screen: {e}");
        }
    }
    close(&mut child, pid);
    drop(board);
    let first = played?;
    // Searched as the session showed it (`shown_log`), the raw log left in
    // the scratch root, which goes on success.
    let shown = shown_log(&at.pty_log(), &at.home(), &at.root.join("pty.shown.log"))?;
    leaks(&shown, &at.root)?;
    check_printed(&prep.faces, &at.pty_log())?;
    if let Some(f) = forbidden {
        f.search(std::slice::from_ref(&shown))?;
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
        scenes,
        started,
        journal,
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
fn play(
    ev: &Events,
    pid: i32,
    at: &Layout,
    fixed: Fixed,
    board: &mac::Kept<mac::General>,
    scenes: &mut Vec<(Instant, String)>,
    steps: &[Step],
) -> Result<Rect, String> {
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
                     and the session crops to where it was first seen, so no one crop holds it. \
                     A window sliding in from a Space switch likely reads this way: start from \
                     a normal desktop, not a full-screen Space"
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
    // Where in the pty log the last typing began, which a picture is
    // looked for after.
    let mut typed_from = 0;
    for &step in steps {
        container::interrupted(&container::INTERRUPTED)?;
        match step {
            Step::Scene(_) => ev.line(&session::event(step, prompt)),
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
                scenes.push((Instant::now(), t.to_string()));
                let ks = keys::keys_for(t).map_err(|c| format!("no key types {c:?}"))?;
                let mut gaps = Gaps::new(seed, ks.len() + 1);
                log.refresh();
                let from = log.bytes.len();
                typed_from = from;
                type_keys(&ks, &mut gaps)?;
                let what = format!("the echo of `{t}` (are the keys arriving?)");
                log.wait(ECHO_LIMIT, &what, |l| session::echoed(&l.bytes, from, t))?;
                let g = gaps.next_ms(session::KEY_GAP_MS) - session::key_hold_ms();
                std::thread::sleep(Duration::from_secs_f64(g.max(0.0) / 1000.0));
                type_keys(&[RETURN], &mut gaps)?;
            }
            Step::Keys(_)
            | Step::Line(_)
            | Step::Escape
            | Step::Ctrl(_)
            | Step::Arrow(_)
            | Step::Paste => {
                ev.line(&session::event(step, prompt));
                log.refresh();
                typed_from = log.bytes.len();
                let ks = keys::presses(step).map_err(|c| format!("no key types {c:?}"))?;
                type_keys(&ks, &mut Gaps::new(seed, ks.len()))?;
            }
            Step::Copy(_) => {
                ev.line(&session::event(step, prompt));
                let ks = keys::presses(step).map_err(|c| format!("no key types {c:?}"))?;
                let before = board.count();
                type_keys(&ks, &mut Gaps::new(seed, ks.len()))?;
                // What the copy wrote is the film's to undo.
                board.film_copied(before);
            }
            Step::Peek(ms) => {
                ev.line(&session::event(step, prompt));
                poster.key(keys::COMMAND, Duration::from_millis(u64::from(ms)))?;
            }
            Step::Picture => {
                log.wait(PICTURE_LIMIT, "the program's first picture", |l| {
                    session::pictured(&l.bytes, typed_from)
                })?;
                ev.line(&session::event(step, prompt));
            }
            Step::Hold(ms) => {
                ev.line(&session::event(step, prompt));
                std::thread::sleep(Duration::from_millis(u64::from(ms)));
            }
            Step::Wheel { turns, lines } => {
                let c = guard::centre(pid, &mac::windows(true))
                    .ok_or("baud has no window on screen")?;
                poster.wheel(c, turns, lines)?;
                ev.line(&session::event(step, prompt));
            }
            Step::Hover(cell) => {
                let w =
                    guard::frame(pid, &mac::windows(true)).ok_or("baud has no window on screen")?;
                let at = session::in_window(cell, session::MIDDLE, (w.x, w.y), (w.w, w.h));
                poster.hover(at)?;
                ev.line(&session::event(step, prompt));
            }
            Step::CmdClick(cell) => {
                let w =
                    guard::frame(pid, &mac::windows(true)).ok_or("baud has no window on screen")?;
                let at = session::in_window(cell, session::MIDDLE, (w.x, w.y), (w.w, w.h));
                log.refresh();
                typed_from = log.bytes.len();
                poster.cmd_click(at)?;
                ev.line(&session::event(step, prompt));
            }
            Step::Drag { from, to } => {
                let w =
                    guard::frame(pid, &mac::windows(true)).ok_or("baud has no window on screen")?;
                let place = |c, within| session::in_window(c, within, (w.x, w.y), (w.w, w.h));
                let (press, release) = session::drag_ends();
                let before = board.count();
                poster.drag(place(from, press), place(to, release))?;
                // What letting go copied is the film's to undo.
                board.film_copied(before);
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

/// **Nothing baud was sent names this machine**, the scratch directory in
/// every spelling and its own name alone included
/// ([`names::with_scratch_leaf`]): what the programs printed is on film. The
/// repository's address, which the README prints, aside.
fn leaks(log: &Path, root: &Path) -> Result<(), String> {
    let bytes = fs::read(log).map_err(|e| format!("{}: {e}", log.display()))?;
    let n = names::with_scratch_leaf(machine_names(root)?, root);
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
    use std::fs;
    use std::path::Path;

    /// **The demo's mpv is the flake's, at [`MPV_REV`], on the Mac's
    /// nixpkgs**: no mpv among nixpkgs' [`PROGRAMS`]; the flake's
    /// [`MPV_OUTPUT`] imports `nix/demo-mpv` with the input pinned at
    /// [`NIXPKGS`]'s revision; that file fetches mpv at [`MPV_REV`] with a
    /// hash, and its build fails unless the source holds both fixes, the
    /// status line's lock and the ST after vo_kitty's delete. The control:
    /// the file with another revision is caught. Perturbation: put `mpv`
    /// back in `PROGRAMS` and the first assertion fails.
    #[test]
    fn the_demos_mpv_is_the_flakes_at_both_fixes() {
        use super::{MPV_OUTPUT, MPV_REV, NIXPKGS, PROGRAMS};
        assert!(
            PROGRAMS.iter().all(|(attr, _)| !attr.starts_with("mpv")),
            "mpv comes from the flake, not from nixpkgs as it is"
        );
        let flake = include_str!("../../../flake.nix");
        let nix = include_str!("../../../nix/demo-mpv/default.nix");
        let rev = NIXPKGS.rsplit('/').next().unwrap();
        assert!(flake.contains(&format!(
            "nixpkgs-demo-mac.url = \"github:NixOS/nixpkgs/{rev}\";"
        )));
        assert!(flake.contains(&format!(
            "aarch64-darwin.{MPV_OUTPUT} = import ./nix/demo-mpv {{\n            \
             pkgs = nixpkgs-demo-mac.legacyPackages.aarch64-darwin;"
        )));
        let pinned = |n: &str| n.contains(&format!("  rev = \"{MPV_REV}\";\n"));
        assert!(pinned(nix), "mpv at {MPV_REV}");
        assert!(
            !pinned(&nix.replace(MPV_REV, &"0".repeat(40))),
            "the control"
        );
        assert!(nix.contains("    hash = \"sha256-"));
        for check in [
            "grep -q '^void terminal_lock_output(void);$' osdep/terminal.h",
            "grep -A1 'append_passthrough(p, out, KITTY_ESC_DELETE_ALL);' video/out/vo_kitty.c",
            "| grep -q 'append_passthrough(p, out, KITTY_ESC_END);'",
        ] {
            assert!(nix.contains(check), "the build checks: {check}");
        }
    }

    /// **Every program names the one output `nix_out` takes**, `out`, so a
    /// package with a second installed output, as yt-dlp's `man` at
    /// `NIXPKGS`, still builds to one path. The control: a bare attribute
    /// is caught. Perturbation: name `yt-dlp` bare and the check fails.
    #[test]
    fn every_program_names_its_out_output() {
        let one = |attr: &str| attr.ends_with("^out");
        for (attr, _) in super::PROGRAMS {
            assert!(one(attr), "{attr} names no output");
        }
        assert!(!one("yt-dlp"), "the control");
    }

    /// **The repository's copy is exactly its paths, at `HEAD`**: the
    /// archive of the repository this test runs in holds each file of
    /// `scratch::REPO_COPY`, a folder's every file at `HEAD`, and nothing
    /// else; each unpacked is `HEAD`'s bytes; the arguments name `HEAD`
    /// and the paths alone. The control: the README unpacked is the one
    /// `git show HEAD:README.md` gives. Perturbation: name anything but
    /// `HEAD`, and the arguments' check fails.
    #[test]
    fn the_repository_copy_is_exactly_its_paths_at_head() {
        let a = super::archive_args();
        assert_eq!(a[..4], ["archive", "--format=tar", "HEAD", "--"]);
        assert_eq!(a[4..], crate::scratch::REPO_COPY);
        let repo = crate::provenance::repository();
        let dest = std::env::temp_dir().join(format!("demo-repo-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        let mut got = super::copy_repo(&repo, &dest).unwrap();
        got.sort();
        let mut want: Vec<String> = Vec::new();
        for p in crate::scratch::REPO_COPY {
            match p.strip_suffix('/') {
                Some(dir) => {
                    let out = std::process::Command::new("git")
                        .arg("-C")
                        .arg(&repo)
                        .args(["ls-tree", "-r", "--name-only", "HEAD", "--", dir])
                        .output()
                        .unwrap();
                    let files = String::from_utf8(out.stdout).unwrap();
                    assert!(!files.is_empty(), "{dir} at HEAD");
                    want.extend(files.lines().map(str::to_string));
                }
                None => want.push(p.to_string()),
            }
        }
        want.sort();
        assert_eq!(got, want);
        let readme = std::fs::read(dest.join("README.md")).unwrap();
        assert_eq!(
            readme,
            super::at_head(&repo, "README.md").unwrap(),
            "the control"
        );
        std::fs::remove_dir_all(&dest).unwrap();
    }

    /// **The leak search refuses the scratch directory however it is
    /// spelled**: as the root is made, through the temporary directory's own
    /// spelling, and by its name alone past a prompt's cut; and it passes
    /// what names none of them, so it is not refusing everything.
    /// Perturbation: search the machine's names alone, and the last spelling
    /// passes.
    #[test]
    fn the_leak_search_refuses_the_scratch_directory_however_spelled() {
        let temp = std::env::temp_dir();
        let root = crate::scratch::fresh_root(&temp).unwrap();
        let leaf = root.file_name().unwrap().to_string_lossy().into_owned();
        let log = root.join("log");
        let says = |text: &str| {
            std::fs::write(&log, text).unwrap();
            super::leaks(&log, &root)
        };
        assert_eq!(says("~/x \u{276F} "), Ok(()));
        for spelled in [
            format!("{}/x", root.display()),
            format!("{}/x", temp.join(&leaf).display()),
            format!("\u{22EF}/{leaf}/x \u{276F} "),
        ] {
            assert!(says(&spelled).is_err(), "{spelled}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// **The demo arms, starts and reads baud's full-screen reading by the
    /// names baud uses**, each the app's constant as its source spells it.
    /// The control: a name of the app's not spelt so is caught.
    #[test]
    fn the_full_screen_reading_is_named_as_baud_names_it() {
        let app = include_str!("../../src/full_screen_check.rs");
        for (name, value) in [
            ("ENV", super::FULL_SCREEN_ENV),
            ("GO", super::FULL_SCREEN_GO),
            ("OUT", super::FULL_SCREEN_OUT),
        ] {
            let line = format!("pub(crate) const {name}: &str = \"{value}\";");
            assert!(app.contains(&line), "{line}");
        }
        assert!(
            !app.contains("pub(crate) const ENV: &str = \"BAUD_FULLSCREEN\";"),
            "the control"
        );
        assert_eq!(
            super::full_screen_name("trailer.gif"),
            "trailer.full-screen.json"
        );
        assert_eq!(super::full_screen_name("tour.mp4"), "tour.full-screen.json");
        let take = include_str!("../../src/take.rs");
        let line = format!("pub(crate) const ENV: &str = \"{}\";", super::TAKE_ENV);
        assert!(take.contains(&line), "{line}");
    }

    /// **The session is asked before the VM and before the window**: in
    /// `run`, at the top and again after everything is built, in `live` before
    /// the recorder or baud starts. These are the calls of a function this
    /// file alone wires, and neither `run` nor `live` can run in a test: they
    /// start a VM and open a window, so the source is what is read. The
    /// control: the slices hold the calls they are searched for after.
    /// Perturbation: delete any `clear_to_open()?;`, and it fails.
    #[test]
    fn the_session_is_asked_before_the_vm_and_the_window() {
        let source = include_str!("run.rs");
        let between = |from: &str, to: &str| {
            let a = source.find(from).expect(from);
            let b = a + source[a..].find(to).expect(to);
            &source[a..b]
        };
        const ASK: &str = "clear_to_open()?;";
        let early = between("fn run(args", "let prep = prepare()?;");
        assert!(early.contains(ASK), "before anything is built");
        let late = between("let forbidden = match (&args.forbidden", "container::up(");
        assert!(late.contains(ASK), "after it is built, before the VM");
        let window = between("fn live(", "record::start(");
        assert!(window.contains(ASK), "before the recorder and baud");
    }

    /// **Only a starship outside both advisories runs**: 1.27.0 and later
    /// pass, 1.26.0 and 1.24.2 -- the demo's other revision's and the
    /// flake's -- are refused, and so is output that names no version.
    #[test]
    fn only_a_starship_outside_the_advisories_runs() {
        for ok in [
            "starship 1.27.0\nbranch:\n",
            "starship 1.28.1",
            "starship 2.0.0",
        ] {
            assert!(super::starship_new_enough(ok), "{ok}");
        }
        for no in [
            "starship 1.26.0\n",
            "starship 1.24.2",
            "",
            "starship",
            "1.27.0",
        ] {
            assert!(!super::starship_new_enough(no), "{no:?}");
        }
    }

    /// **The recorded command names neither the forbidden list nor its
    /// controls**, whose paths are this machine's, and keeps every other
    /// argument as typed. The control: an argument after another flag is
    /// kept.
    #[test]
    fn the_recorded_command_keeps_the_list_out() {
        let args = [
            "--record",
            "assets/demo/trailer.gif",
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
                "assets/demo/trailer.gif",
                "--forbidden",
                "LIST",
                "--controls",
                "FILE"
            ]
        );
    }
    /// **Take 2 is refused without the tour's address and its stretch**:
    /// `--trailer` with no `--tour`, an address that is not a YouTube
    /// video's, or no `--start` or `--end`, is refused before anything is
    /// fetched, built or opened, and `--tour`, `--start` or `--end` without
    /// `--trailer` too; with all three it records the README's GIF and
    /// nothing else, and the tour an MP4 and nothing else. The control: the
    /// same command with all three passes. Perturbation: drop the
    /// `session::trailer` check from `parse` and the first assertion fails.
    #[test]
    fn take_2_refuses_without_the_tours_address() {
        let parse = |a: &[&str]| super::parse(a.iter().map(|s| (*s).to_string())).map(|_| ());
        let list = ["--forbidden", "lists/f.txt", "--controls", "lists/c.txt"];
        let with = |a: &[&'static str]| -> Vec<&'static str> { [a, &list[..]].concat() };
        let gif = ["--trailer", "--record", "assets/demo/trailer.gif"];
        let span = ["--start", "1:23.5", "--end", "1:33"];
        let url = ["--tour", "https://youtu.be/abcdefghijk"];
        let err = parse(&with(&[&gif[..], &span[..]].concat())).unwrap_err();
        assert!(err.contains("--tour"), "{err}");
        let other = ["--tour", "https://example.com/v"];
        let err = parse(&with(&[&gif[..], &other[..], &span[..]].concat())).unwrap_err();
        assert!(err.contains("not a YouTube address"), "{err}");
        for half in [&span[..2], &span[2..], &[][..]] {
            let err = parse(&with(&[&gif[..], &url[..], half].concat())).unwrap_err();
            assert!(err.contains("--start") && err.contains("--end"), "{err}");
        }
        let all = [&gif[..], &url[..], &span[..]].concat();
        assert_eq!(parse(&with(&all)), Ok(()), "the control");
        assert!(
            parse(&["--trailer", "--dry"]).is_err(),
            "dry, still refused"
        );
        assert!(parse(&url).is_err(), "--tour without --trailer");
        assert!(parse(&span).is_err(), "--start and --end without --trailer");
        let mp4 = ["--trailer", "--record", "assets/demo/trailer.mp4"];
        let mp4 = [&mp4[..], &url[..], &span[..]].concat();
        assert!(parse(&with(&mp4)).is_err(), "the trailer is a GIF");
        let tour_gif = ["--record", "assets/demo/tour.gif"];
        assert!(parse(&with(&tour_gif)).is_err(), "the tour is an MP4");
        assert_eq!(
            parse(&with(&["--record", "assets/demo/tour.mp4"])),
            Ok(()),
            "the tour's take"
        );
    }

    /// **The flash gate is given an absolute path and counts that its one
    /// test ran**: a relative path is refused before anything runs, as the
    /// census reads it from `baud-bench/`; an absolute one is passed on as
    /// it is; and only libtest's `1 passed` passes, not a run of none.
    /// Perturbation: drop the relative path's refusal and its assertion
    /// fails.
    #[test]
    fn the_flash_gate_reads_an_absolute_path_and_one_test() {
        let p = super::Profile {
            dir: std::path::PathBuf::from("/t/release"),
            name: "release".to_string(),
            target: std::path::PathBuf::from("/t"),
            triple: None,
        };
        let relative = std::path::Path::new("assets/demo/trailer.gif");
        let err = super::gate_command(&p, relative).map(|_| ()).unwrap_err();
        assert!(err.contains("absolute"), "{err}");
        let absolute = std::path::Path::new("/s/baud-demo-1-0/trailer.gif");
        let cmd = super::gate_command(&p, absolute).unwrap();
        let env: Vec<_> = cmd.get_envs().collect();
        assert!(
            env.contains(&(
                std::ffi::OsStr::new("BAUD_TRAILER_GIF"),
                Some(absolute.as_os_str())
            )),
            "{env:?}"
        );
        assert!(super::ran_one(
            "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored\n"
        ));
        assert!(!super::ran_one(
            "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored\n"
        ));
        assert!(!super::ran_one("test result: FAILED. 0 passed; 1 failed\n"));
    }

    /// **What a session showed is searched, at both call sites, and a
    /// folder report's scratch path is not what it showed**: the bytes the
    /// headless run and the live take search ([`super::shown_log`]) hold
    /// each shell folder report as the title shows its folder, so a scratch
    /// path inside one names nothing, while the same path printed still
    /// does. A heard log and a pty log alike. The control: the raw bytes
    /// name the root. Named break: search the raw bytes.
    #[test]
    fn leaks_reads_what_the_session_showed() {
        let root = std::env::temp_dir().join(format!("baud-demo-shown-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("home");
        fs::create_dir_all(home.join("src")).expect("the scratch home");
        let real = fs::canonicalize(&home).expect("the home");
        let report = format!("\x1b]7;file://example-host{}/src\x07ok\r\n", real.display());
        for raw_name in ["heard.log", "pty.log"] {
            let raw = root.join(raw_name);
            fs::write(&raw, &report).expect("the raw log");
            assert!(
                super::leaks(&raw, &root).is_err(),
                "the control: the raw log names the root"
            );
            let shown = super::shown_log(&raw, &home, &root.join("shown.log")).expect("shown");
            assert_eq!(
                super::leaks(&shown, &root),
                Ok(()),
                "{raw_name}: a report shows as ~/src"
            );
            fs::write(&raw, format!("{}/src\r\n", real.display())).expect("printed");
            let shown = super::shown_log(&raw, &home, &root.join("shown.log")).expect("shown");
            assert!(
                super::leaks(&shown, &root).is_err(),
                "{raw_name}: a printed path is found"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// The references a Markdown or HTML text makes: each `](target)`, and
    /// each `src`, `href` and `srcset` value, a `srcset`'s every entry.
    fn references(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (at, _) in text.match_indices("](") {
            let rest = &text[at + 2..];
            if let Some(end) = rest.find(')') {
                out.push(
                    rest[..end]
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string(),
                );
            }
        }
        for attr in ["src=\"", "href=\"", "srcset=\""] {
            for (at, _) in text.match_indices(attr) {
                let rest = &text[at + attr.len()..];
                let Some(end) = rest.find('"') else { continue };
                for entry in rest[..end].split(',') {
                    out.push(entry.split_whitespace().next().unwrap_or("").to_string());
                }
            }
        }
        out
    }

    /// What the README must never point at: the raw and shown logs, a
    /// stills folder, the link layer's dumps, cargo's target directory.
    const UNPUBLISHED: [&str; 8] = [
        "heard.log",
        "heard.shown.log",
        "heard.raw.log",
        "pty.log",
        "pty.shown.log",
        "stills",
        "links/",
        "target/",
    ];

    /// **Nothing the README references, nor any document it leads to in
    /// this repository, points at a session's logs, a stills folder or the
    /// link layer's dumps**: those stay on the machine that made them, and
    /// what is published is the film and the frames chosen from it. The
    /// site lives outside this repository: this lint does not read it, and
    /// it is covered by the rule that no session log is ever tracked
    /// ([`no_session_output_is_tracked`]). The controls: the README's own
    /// references are read, and a reference to a still or a dump is caught.
    /// Named break: an empty list.
    #[test]
    fn the_readme_points_at_no_session_log() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let caught = |r: &str| UNPUBLISHED.iter().any(|u| r.contains(u));
        let made = "![a still](demo/run11/stills/02-launch.png) [dump](run11/links/05-step.txt)";
        assert!(
            references(made).iter().all(|r| caught(r)),
            "the control: {made}"
        );
        let mut todo = vec![root.join("README.md")];
        let mut seen = std::collections::BTreeSet::new();
        let mut found = Vec::new();
        while let Some(doc) = todo.pop() {
            let Ok(real) = fs::canonicalize(&doc) else {
                continue;
            };
            if !seen.insert(real.clone()) {
                continue;
            }
            let text = fs::read_to_string(&real).unwrap_or_default();
            for r in references(&text) {
                if caught(&r) {
                    found.push(format!("{}: {r}", doc.display()));
                }
                let local = r.split('#').next().unwrap_or("");
                if local.ends_with(".md") && !local.contains("://") {
                    todo.push(real.parent().expect("a folder").join(local));
                }
            }
        }
        assert!(
            seen.len() > 1,
            "the control: the README leads to its documents"
        );
        assert_eq!(found, Vec::<String>::new());
    }

    /// What a session leaves that is never tracked: a heard or pty log in
    /// any form, the link layer's dumps (a `links` folder), a stills folder,
    /// and the film with its lists.
    fn session_output(path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        let log = |stem: &str| name.starts_with(stem) && name.ends_with(".log");
        let folder = |dir: &str| path.split('/').rev().skip(1).any(|s| s == dir);
        log("heard")
            || log("pty")
            || folder("links")
            || folder("stills")
            || ["session.mkv", "session.mkv.shots", "session.mkv.steps"].contains(&name)
    }

    /// The tracked paths of `listing`, `git ls-files -z`'s, that are a
    /// session's output.
    fn tracked_outputs(listing: &[u8]) -> Vec<String> {
        listing
            .split(|&b| b == 0)
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .filter(|p| session_output(p))
            .collect()
    }

    /// **No session's output is ever tracked**: no heard or pty log, no
    /// link layer dump and no stills folder or film is in the repository's
    /// index, so nothing that reads the repository -- the README, the site,
    /// any other consumer -- can publish one. Raw logs only ever go to
    /// cargo's gitignored target directory and to scratch folders. The
    /// controls: a listing holding each kind is caught, and the repository's
    /// own listing is read. Named break: match nothing.
    ///
    /// **It needs a git checkout and `git` on `PATH`**: it reads the index
    /// with `git ls-files`. The suite runs in the dev shell; the flake's
    /// build runs no cargo tests (`doCheck = false`), and its one check
    /// reads the linked SDK, so no nix sandbox without `.git` runs it. Run
    /// from a tree without one, it fails at `git ls-files`, and an empty
    /// listing fails the size control: it never passes on no listing.
    #[test]
    fn no_session_output_is_tracked() {
        let fixture: &[u8] = b"README.md\0demo/heard.log\0x/heard.raw.log\0run/pty.shown.log\0\
            run11/links/05-step.txt\0run11/stills/02-launch.png\0run11/session.mkv.steps\0\
            tests/completion/zsh.steps\0baud-app/examples/demo/links.toml\0";
        assert_eq!(
            tracked_outputs(fixture),
            [
                "demo/heard.log",
                "x/heard.raw.log",
                "run/pty.shown.log",
                "run11/links/05-step.txt",
                "run11/stills/02-launch.png",
                "run11/session.mkv.steps",
            ],
            "the control: each kind is caught, and the rest is not"
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["ls-files", "-z"])
            .output()
            .expect("git");
        assert!(out.status.success(), "git ls-files");
        assert!(
            out.stdout.split(|&b| b == 0).count() > 100,
            "the control: the repository's files are listed"
        );
        assert_eq!(tracked_outputs(&out.stdout), Vec::<String>::new());
    }
}
