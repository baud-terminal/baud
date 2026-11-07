//! **`--record`: the session filmed and encoded by the run that plays it.**
//! The recorder, `scripts/trail-record.swift` compiled with Xcode's
//! `swiftc`, starts before baud does and films the main display, every
//! application already running left out; baud is launched once it films,
//! so the film holds the launch from its first frame. When the session
//! ends, the film is cropped to baud's window, sampled at 30 frames a
//! second by ffmpeg and encoded by gifski with exactly
//! [`GIFSKI`](crate::provenance::GIFSKI), both from the pinned nixpkgs;
//! nothing is trimmed, padded or sped up. The recorder ends the film at
//! the stop and says where, and the film must play to there and no
//! further. The GIF and its provenance file reach the path given only once
//! every check has passed.
//!
//! **Or an MP4**, when the path given ends `.mp4`: the same film cropped
//! to the same window, sampled at [`film::MP4_FPS`] and encoded by the
//! pinned ffmpeg's libx264 with [`film::MP4_ENCODE`], so the same film makes
//! the same bytes; it must play from the film's first frame to its stop, as
//! many frames as that runs, starting at 0. Beside it the take's timeline,
//! `NAME.timeline.tsv` (`timeline.rs`): baud, built with the `take`
//! feature and started with `BAUD_TAKE`,
//! journals what it showed on the host clock, the recorder says where the
//! film starts on that clock, and the demo adds each typed command; the
//! timeline is refused if it names this machine. Beside both,
//! `NAME.mp4.provenance.toml` (`provenance::tour_document`): the commit
//! filmed, its tree, the features baud was built with, the command, the
//! MP4's and the timeline's SHA-256, every step with its time, and the
//! video's description -- an end card naming the commit and the feature,
//! and the credit line of `assets/demo/LICENSES.md`. `--verify` is the
//! GIF's: the MP4 itself is uploaded, not kept in the repository.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::film::{self, Crop, Display};
use crate::guard::{self, Console, Rect};
use crate::names;
use crate::provenance::{self, Facts, Tree};
use crate::run::{Events, NIXPKGS};
use crate::scratch::Layout;
use crate::timeline;

/// Where Xcode is: the recorder is compiled with its `swiftc` and SDK, as
/// `scripts/readme-capture.sh` compiles it.
const XCODE: &str = "/Applications/Xcode.app/Contents/Developer";

/// How a path in the scratch directory is written in the provenance file.
pub const SCRATCH: &str = "<scratch>";

/// What `--record` makes, by the path's extension.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Gif,
    Mp4,
}

impl Form {
    /// The form a path names: `.gif` or `.mp4`, with a name before it.
    pub fn of(name: &str) -> Option<Form> {
        match name.rsplit_once('.') {
            Some((stem, "gif")) if !stem.is_empty() => Some(Form::Gif),
            Some((stem, "mp4")) if !stem.is_empty() => Some(Form::Mp4),
            _ => None,
        }
    }
}

/// **What `--record` needs, resolved before any window**: where the GIF
/// or the MP4 goes, the encoder and the compiled recorder.
pub struct Kit {
    pub out: PathBuf,
    /// The output's file name, `.gif` or `.mp4`.
    pub gif: String,
    pub form: Form,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub gifski: PathBuf,
    /// The encoder's nixpkgs attributes and store paths.
    pub tools: Vec<(String, String)>,
    pub recorder: PathBuf,
    /// The compile command, the scratch directory written [`SCRATCH`].
    pub compile: Vec<String>,
    /// `swiftc --version`.
    pub swiftc: Vec<String>,
    pub source_sha256: String,
    /// Xcode's `swiftc`, its macOS SDK and the recorder's source, for the
    /// writer check's build.
    swiftc_bin: PathBuf,
    sdk: PathBuf,
    source: PathBuf,
}

/// `source` compiled by Xcode's `swiftc` into `out`, `defines` set.
fn compile(
    swiftc: &Path,
    sdk: &Path,
    source: &Path,
    out: &Path,
    defines: &[&str],
) -> Result<(), String> {
    let mut c = Command::new(swiftc);
    c.env("DEVELOPER_DIR", XCODE).env_remove("SDKROOT");
    c.args(["-O", "-suppress-warnings"]);
    for d in defines {
        c.args(["-D", d]);
    }
    c.arg("-sdk").arg(sdk).arg("-o").arg(out).arg(source);
    run_ok(&mut c, "swiftc scripts/trail-record.swift")
}

fn run_ok(cmd: &mut Command, what: &str) -> Result<(), String> {
    let st = cmd.status().map_err(|e| format!("{what}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({st})"))
    }
}

fn say(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// **The kit**: the GIF's path checked, gifski and ffmpeg from the pinned
/// nixpkgs, the recorder compiled into the scratch directory.
pub fn kit(
    out: &Path,
    at: &Layout,
    root: &Path,
    nix_out: impl Fn(&str) -> Result<PathBuf, String>,
) -> Result<Kit, String> {
    let gif = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| Form::of(n).is_some())
        .ok_or_else(|| format!("--record takes a .gif or .mp4 path, not {}", out.display()))?;
    let form = Form::of(&gif).unwrap_or(Form::Gif);
    let dir = match out.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let mut tools = Vec::new();
    let mut store = |attr: &str| -> Result<PathBuf, String> {
        let p = nix_out(attr)?;
        tools.push((attr.to_string(), p.to_string_lossy().into_owned()));
        Ok(p)
    };
    let gifski = store("gifski")?.join("bin/gifski");
    let ff = store("ffmpeg-headless.bin")?.join("bin");
    let (ffmpeg, ffprobe) = (ff.join("ffmpeg"), ff.join("ffprobe"));
    for exe in [&gifski, &ffmpeg, &ffprobe] {
        if !exe.is_file() {
            return Err(format!("no {}", exe.display()));
        }
    }
    println!("{:<13}{}", "gifski", gifski.display());
    println!("{:<13}{}", "ffmpeg", ffmpeg.display());

    let source = root.join("scripts/trail-record.swift");
    let swiftc = Path::new(XCODE).join("Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc");
    let sdk = Path::new(XCODE).join("Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk");
    if !swiftc.is_file() || !sdk.is_dir() {
        return Err(format!(
            "--record compiles its recorder with Xcode's swiftc and macOS SDK, under {XCODE}, \
             and they are not there"
        ));
    }
    let recorder = at.root.join("trail-record");
    compile(&swiftc, &sdk, &source, &recorder, &[])?;
    let mut v = Command::new(&swiftc);
    v.env("DEVELOPER_DIR", XCODE).env_remove("SDKROOT");
    v.arg("--version");
    let version = say(&mut v, "swiftc --version")?;
    let compile = vec![
        swiftc.to_string_lossy().into_owned(),
        "-O".to_string(),
        "-suppress-warnings".to_string(),
        "-sdk".to_string(),
        sdk.to_string_lossy().into_owned(),
        "-o".to_string(),
        format!("{SCRATCH}/trail-record"),
        "scripts/trail-record.swift".to_string(),
    ];
    let swiftc_said: Vec<String> = version.lines().map(str::to_string).collect();
    println!(
        "{:<13}scripts/trail-record.swift, compiled by {}",
        "recorder",
        swiftc_said.first().map_or("swiftc", String::as_str)
    );
    Ok(Kit {
        out: out.to_path_buf(),
        gif,
        form,
        ffmpeg,
        ffprobe,
        gifski,
        tools,
        recorder,
        compile,
        swiftc: swiftc_said,
        source_sha256: provenance::sha256(&source)?,
        swiftc_bin: swiftc,
        sdk,
        source,
    })
}

/// **The recorder, filming**: its process, what it printed, a line each,
/// and the display it films.
pub struct Filming {
    child: Child,
    said: Arc<Mutex<Vec<String>>>,
    /// When the line saying where the film starts on the host clock was
    /// read: the demo's clock carried onto the host's (`timeline::Clock`).
    started: Arc<Mutex<Option<Instant>>>,
    err: Arc<Mutex<String>>,
    threads: Vec<JoinHandle<()>>,
    pub display: Display,
}

/// **The recorder started and filming**: each line it prints goes to the
/// events log as it comes; this returns once it has created `ready`, so
/// baud is launched only after the film has begun. One that stops first
/// -- Screen Recording not allowed, say -- is reported, and nothing has
/// been opened.
pub fn start(kit: &Kit, at: &Layout, ev: &Events) -> Result<Filming, String> {
    let dir = at.root.to_string_lossy().into_owned();
    let mut child = Command::new(&kit.recorder)
        .args(film::recorder_args(&dir))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("the recorder: {e}"))?;
    ev.line("recorder started");
    let said = Arc::new(Mutex::new(Vec::new()));
    let started = Arc::new(Mutex::new(None));
    let err = Arc::new(Mutex::new(String::new()));
    let mut threads = Vec::new();
    if let Some(out) = child.stdout.take() {
        let (said, ev, at) = (Arc::clone(&said), ev.clone(), Arc::clone(&started));
        threads.push(std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if line.starts_with("film starts at host ") {
                    if let Ok(mut a) = at.lock() {
                        *a = Some(Instant::now());
                    }
                }
                ev.line(&format!("recorder: {line}"));
                if let Ok(mut s) = said.lock() {
                    s.push(line);
                }
            }
        }));
    }
    if let Some(e) = child.stderr.take() {
        let err = Arc::clone(&err);
        threads.push(std::thread::spawn(move || {
            for line in BufReader::new(e).lines().map_while(Result::ok) {
                if let Ok(mut s) = err.lock() {
                    s.push_str(&line);
                    s.push('\n');
                }
            }
        }));
    }
    let mut f = Filming {
        child,
        said,
        started,
        err,
        threads,
        display: Display {
            pt: (0.0, 0.0),
            scale: 0.0,
            px: (0, 0),
        },
    };
    let ready = at.root.join("ready");
    let t = Instant::now();
    while !ready.exists() {
        if let Ok(Some(st)) = f.child.try_wait() {
            f.join();
            return Err(format!(
                "the recorder stopped before it filmed ({st}): {}. It films through \
                 ScreenCaptureKit, which macOS allows only an application trusted for \
                 Screen Recording: allow the terminal this runs in under System Settings > \
                 Privacy & Security > Screen & System Audio Recording. baud was not opened.",
                f.err().trim()
            ));
        }
        if t.elapsed() > Duration::from_secs(30) {
            f.stop();
            return Err("the recorder never began to film".to_string());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    ev.line("recorder ready");
    let t = Instant::now();
    loop {
        let d = f
            .said
            .lock()
            .ok()
            .and_then(|s| s.iter().find_map(|l| film::display(l)));
        if let Some(d) = d {
            f.display = d;
            return Ok(f);
        }
        if t.elapsed() > Duration::from_secs(3) {
            f.stop();
            return Err("the recorder never named the display it films".to_string());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

impl Filming {
    /// When the film's start on the host clock was read, once it has been.
    pub fn started(&self) -> Option<Instant> {
        self.started.lock().ok().and_then(|a| *a)
    }

    fn err(&self) -> String {
        self.err.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn join(&mut self) {
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }

    /// The recorder stopped at once, by its pid.
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join();
    }

    /// **The recorder's end**: it stops [`film::TAIL`] seconds after
    /// `done`, finishes the file and exits; this waits up to `limit` and
    /// returns what it printed, or why it refused the film -- baud not
    /// frontmost at the stop, the writer incomplete, frames dropped -- or
    /// why [`guard::console`] refuses it now: a film that ended on a lock
    /// screen or a sleeping display shows that, and no GIF is made of it.
    pub fn finish(
        mut self,
        limit: Duration,
        console: impl Fn() -> Console,
    ) -> Result<Vec<String>, String> {
        let t = Instant::now();
        let st = loop {
            match self.child.try_wait() {
                Ok(Some(st)) => break st,
                Ok(None) if t.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    self.stop();
                    return Err("the recorder did not finish the film".to_string());
                }
            }
        };
        self.join();
        if !st.success() {
            return Err(format!(
                "the recorder refused the film ({st}): {}",
                self.err().trim()
            ));
        }
        guard::console(&console()).map_err(|r| {
            format!(
                "REFUSED at the end of the film: {}; no GIF was written",
                r.0
            )
        })?;
        Ok(self.said.lock().map(|s| s.clone()).unwrap_or_default())
    }
}

/// What the encoder made.
pub struct Encoded {
    pub frames: usize,
    pub film: f64,
    pub gif: f64,
}

/// ffprobe asked how long a file plays: `-show_entries format=duration`.
fn duration(kit: &Kit) -> Command {
    let mut c = Command::new(&kit.ffprobe);
    c.args([
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "csv=p=0",
    ]);
    c
}

fn seconds(text: &str, what: &str) -> Result<f64, String> {
    text.trim()
        .parse::<f64>()
        .map_err(|_| format!("ffprobe gave {what} as {text:?}"))
}

/// **The film made the GIF**: ffmpeg crops it to `c` and samples it at 30
/// frames a second into PNGs under `work`, gifski encodes them with exactly
/// its settings into `gif`, and the GIF must run as long as the film plays
/// ([`film::length`], [`film::covers`]).
pub fn encode(kit: &Kit, raw: &Path, c: &Crop, work: &Path, gif: &Path) -> Result<Encoded, String> {
    let frames = work.join("frames");
    let _ = fs::remove_dir_all(&frames);
    fs::create_dir_all(&frames).map_err(|e| format!("{}: {e}", frames.display()))?;
    let raw_s = raw.to_string_lossy();
    let frames_s = frames.to_string_lossy();
    run_ok(
        Command::new(&kit.ffmpeg).args(film::frames_args(&raw_s, c, &frames_s)),
        "ffmpeg",
    )?;
    let mut pngs: Vec<PathBuf> = fs::read_dir(&frames)
        .map_err(|e| format!("{}: {e}", frames.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    pngs.sort();
    if pngs.is_empty() {
        return Err("ffmpeg made no frames of the film".to_string());
    }
    // gifski's progress bar ends without a newline; what it says is kept
    // for a failure.
    let made = Command::new(&kit.gifski)
        .args(film::gifski_args(&gif.to_string_lossy()))
        .args(&pngs)
        .output()
        .map_err(|e| format!("gifski: {e}"))?;
    if !made.status.success() || !gif.is_file() {
        return Err(format!(
            "gifski failed ({}): {}",
            made.status,
            String::from_utf8_lossy(&made.stderr).trim()
        ));
    }
    let plays = say(duration(kit).arg(raw), "ffprobe")?;
    let film = film::length(&plays)
        .ok_or_else(|| format!("ffprobe gave the film's length as {plays:?}"))?;
    let shown = say(duration(kit).arg(gif), "ffprobe")?;
    let gif_s = seconds(&shown, "the GIF's length")?;
    film::covers(pngs.len(), film, gif_s)?;
    Ok(Encoded {
        frames: pngs.len(),
        film,
        gif: gif_s,
    })
}

/// What the run that played the session hands the provenance file.
pub struct Run<'a> {
    pub root: &'a Path,
    pub command: String,
    pub from: String,
    pub tree: &'a Tree,
    pub tools: &'a [(String, String)],
    pub pins: &'a [(String, String)],
    pub names: &'a [String],
}

fn bytes_of(p: &Path) -> Result<u64, String> {
    fs::metadata(p)
        .map(|m| m.len())
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// The macOS product version, `sw_vers -productVersion`.
fn macos() -> Result<String, String> {
    say(
        Command::new("/usr/bin/sw_vers").arg("-productVersion"),
        "sw_vers",
    )
}

/// `path` with the scratch directory written [`SCRATCH`].
fn scrub(s: &str, at: &Layout) -> String {
    s.replace(&*at.root.to_string_lossy(), SCRATCH)
}

/// **The provenance file's text**, refused if it names this machine:
/// [`names::named_outside`] over every byte, the repository's public
/// address aside, the scratch directory's path among the names.
pub fn text(f: &Facts, names: &[String]) -> Result<String, String> {
    let header = provenance::header(&f.gif);
    let header: Vec<&str> = header.iter().map(String::as_str).collect();
    let text = provenance::document(f).text(&header);
    let public = names::public_path(env!("CARGO_PKG_REPOSITORY"));
    let hit = names::named_outside(text.as_bytes(), names, &[&public]);
    if !hit.is_empty() {
        return Err(format!(
            "the provenance file would name this machine: {hit:?}; give the GIF's path \
             relative to where this runs"
        ));
    }
    Ok(text)
}

#[allow(clippy::too_many_arguments)]
fn facts(
    kit: &Kit,
    run: &Run,
    at: &Layout,
    gif: &Path,
    e: &Encoded,
    raw: &Path,
    raw_kept: String,
    display: String,
    window: String,
    c: &Crop,
    said: Vec<String>,
    events: Vec<String>,
) -> Result<Facts, String> {
    let mut tools = run.tools.to_vec();
    tools.extend(kit.tools.iter().cloned());
    Ok(Facts {
        gif: kit.gif.clone(),
        command: run.command.clone(),
        from: run.from.clone(),
        tree: run.tree.clone(),
        macos: macos()?,
        gif_sha256: provenance::sha256(gif)?,
        gif_bytes: bytes_of(gif)?,
        gif_frames: e.frames,
        gif_seconds: e.gif,
        raw_sha256: provenance::sha256(raw)?,
        raw_bytes: bytes_of(raw)?,
        raw_seconds: e.film,
        raw_kept,
        display,
        window,
        crop: c.filter(),
        recorder_sha256: kit.source_sha256.clone(),
        compile: kit.compile.clone(),
        swiftc: kit.swiftc.clone(),
        recorder_args: film::recorder_args(SCRATCH),
        recorder_said: said.iter().map(|l| scrub(l, at)).collect(),
        frames_args: std::iter::once("ffmpeg".to_string())
            .chain(film::frames_args(
                &format!("{SCRATCH}/raw.mp4"),
                c,
                &format!("{SCRATCH}/frames"),
            ))
            .collect(),
        gifski_args: std::iter::once("gifski".to_string())
            .chain(film::gifski_args(&format!("{SCRATCH}/{}", kit.gif)))
            .collect(),
        gifski_inputs: format!(
            "{SCRATCH}/frames/000001.png to {:06}.png, the {} frames in order",
            e.frames, e.frames
        ),
        nixpkgs: NIXPKGS.rsplit('/').next().unwrap_or(NIXPKGS).to_string(),
        tools,
        pins: run.pins.to_vec(),
        events: events.iter().map(|l| scrub(l, at)).collect(),
    })
}

/// **The film ends at the stop, checked without filming**: the recorder's
/// source built with `-D WRITER_CHECK` -- its writer and its end, with no
/// capture and no window -- writes each of [`film::CHECKS`], and each must
/// play to its stop and no further ([`film::ends_at_stop`]), as the build
/// says and as ffprobe reads it, and, through [`encode`], make as many
/// frames as ffmpeg's fps=30 samples from the length it plays.
fn writer_check(kit: &Kit, dir: &Path) -> Result<(), String> {
    let check = dir.join("trail-record-check");
    compile(
        &kit.swiftc_bin,
        &kit.sdk,
        &kit.source,
        &check,
        &["WRITER_CHECK"],
    )?;
    let c = Crop {
        w: 320,
        h: 200,
        x: 160,
        y: 100,
    };
    let mut seen = Vec::new();
    for (n, (frames, hold)) in film::CHECKS.iter().enumerate() {
        let stop = film::stop_of(*frames, hold).ok_or("a writer check without a stop")?;
        let raw = dir.join(format!("writer-{n}.mp4"));
        let said = say(
            Command::new(&check).args(film::check_args(&raw.to_string_lossy(), *frames, hold)),
            "the writer check",
        )?;
        let said: Vec<String> = said.lines().map(str::to_string).collect();
        let says = film::said_film(&said)
            .ok_or_else(|| format!("the writer check never said where its film stops: {said:?}"))?;
        film::ends_at_stop(says, stop)?;
        let work = dir.join(format!("writer-{n}"));
        let e = encode(kit, &raw, &c, &work, &work.join("writer.gif"))?;
        film::ends_at_stop(e.film, stop)?;
        let want = (stop * f64::from(provenance::FPS)).round() as usize;
        if e.frames != want {
            return Err(format!(
                "a film that stops at {stop:.3} s made {} frames at {}, not {want}",
                e.frames,
                provenance::FPS
            ));
        }
        seen.push(format!(
            "{frames} frames and {hold} s to the stop: plays {:.3} s, {} frames",
            e.film, e.frames
        ));
    }
    println!(
        "check:   the recorder's writer, no capture, ends its film at the stop: {}",
        seen.join("; ")
    );
    Ok(())
}

/// **`--dry --record`'s check of everything after the window**: the
/// recorder's end first ([`writer_check`]), then a film made here: two
/// seconds of ffmpeg's `testsrc` at 60 frames a second, cropped to its
/// middle and encoded by the same gifski line, must give 60 frames; its
/// provenance file must name nothing of this machine and `--verify` must
/// take it; and with one byte of the GIF changed, `--verify` must refuse
/// it -- the control that the check can fail.
pub fn self_check(kit: &Kit, at: &Layout, run: &Run) -> Result<(), String> {
    let dir = at.root.join("check");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    writer_check(kit, &dir)?;
    let raw = dir.join("raw.mp4");
    run_ok(
        Command::new(&kit.ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc=size=640x400:rate=60")
            .args(["-t", "2", "-pix_fmt", "yuv420p"])
            .arg(&raw),
        "ffmpeg testsrc",
    )?;
    let c = Crop {
        w: 320,
        h: 200,
        x: 160,
        y: 100,
    };
    let gif = dir.join(&kit.gif);
    let e = encode(kit, &raw, &c, &dir, &gif)?;
    if e.frames != 60 {
        return Err(format!(
            "2 s at 30 a second made {} frames, not 60",
            e.frames
        ));
    }
    let f = facts(
        kit,
        run,
        at,
        &gif,
        &e,
        &raw,
        "the self-check's testsrc, not kept".to_string(),
        "testsrc 640x400 at 60 frames a second, 2 s".to_string(),
        "160,100 320x200 px".to_string(),
        &c,
        Vec::new(),
        vec!["0.000 testsrc".to_string()],
    )?;
    let text = text(&f, run.names)?;
    fs::write(provenance::path_of(&gif), &text).map_err(|e| format!("{e}"))?;
    let said = provenance::verify(&gif, run.root)?;
    let mut bytes = fs::read(&gif).map_err(|e| format!("{}: {e}", gif.display()))?;
    let mid = bytes.len() / 2;
    bytes[mid] ^= 1;
    fs::write(&gif, &bytes).map_err(|e| format!("{}: {e}", gif.display()))?;
    let changed = provenance::verify(&gif, run.root);
    match &changed {
        Err(why) if why.contains("not the GIF that was recorded") => {}
        Err(why) => {
            return Err(format!(
                "--verify refused a changed GIF for another reason: {why}"
            ));
        }
        Ok(_) => return Err("--verify took a GIF with one byte changed".to_string()),
    }
    println!(
        "check:   ffmpeg's testsrc, 2 s at 60 frames a second, cropped ({}) and encoded by \
         the gifski line: {} frames, {:.3} s for a film of {:.3} s",
        c.filter(),
        e.frames,
        e.gif,
        e.film
    );
    println!(
        "         its provenance file ({} lines) names nothing of this machine; --verify takes it:",
        text.lines().count()
    );
    for l in said {
        println!("           {l}");
    }
    println!(
        "         with byte {mid} of {} changed, --verify refuses it",
        bytes.len()
    );
    if kit.form == Form::Mp4 {
        let (one, two) = (dir.join("check.mp4"), dir.join("again.mp4"));
        let m = encode_mp4(kit, &raw, &c, &one)?;
        encode_mp4(kit, &raw, &c, &two)?;
        if m.frames != 120 {
            return Err(format!(
                "2 s at {} a second made {} frames, not 120",
                film::MP4_FPS,
                m.frames
            ));
        }
        let read = |p: &Path| fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
        if read(&one)? != read(&two)? {
            return Err("two encodes of one film made different MP4s".to_string());
        }
        println!(
            "check:   the same testsrc as an MP4, twice: {} frames, {:.3} s, the same bytes",
            m.frames, m.mp4
        );
    }
    Ok(())
}

/// What the session left for the GIF: its events, what the recorder
/// printed, the display it filmed and baud's window.
pub struct Session {
    pub events: Vec<String>,
    pub said: Vec<String>,
    pub display: Display,
    pub frame: Rect,
    /// For the MP4's timeline: each typed command and when its first key
    /// was posted, when the film's start was read, and baud's journal.
    pub scenes: Vec<(Instant, String)>,
    pub started: Option<Instant>,
    pub journal: PathBuf,
}

/// **The GIF made, checked and written**: the film cropped to baud's
/// window and encoded; the repository still at the commit it was built
/// from; the raw film kept in cargo's target directory; the provenance
/// file checked for this machine's names; then the GIF and its file
/// written beside each other and `--verify` run on them.
///
/// `gate` judges the encoded GIF where it was made, in the scratch
/// directory, before anything is copied out of it: a GIF it refuses never
/// reaches `kit.out` ([`keep_gif`]).
pub fn finish(
    kit: &Kit,
    at: &Layout,
    run: &Run,
    target: &Path,
    s: Session,
    gate: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    if kit.form == Form::Mp4 {
        return finish_mp4(kit, at, run, target, s);
    }
    let raw = at.root.join("raw.mp4");
    let c = film::crop(&s.frame, &s.display)?;
    let gif = at.root.join(&kit.gif);
    println!(
        "encode:  the film cropped to baud's window ({})",
        c.filter()
    );
    let e = encode(kit, &raw, &c, &at.root, &gif)?;
    let stop = film::said_film(&s.said).ok_or("the recorder never said where its film stops")?;
    film::ends_at_stop(e.film, stop)?;
    let now = provenance::tree(run.root)?;
    if now.commit != run.tree.commit {
        return Err(format!(
            "the repository moved from {} to {} while it was filmed",
            run.tree.commit, now.commit
        ));
    }
    let stem = kit.gif.trim_end_matches(".gif");
    let keep_dir = target.join("demo");
    fs::create_dir_all(&keep_dir).map_err(|e| format!("{}: {e}", keep_dir.display()))?;
    let keep = keep_dir.join(format!("{stem}.raw.mp4"));
    let display = s
        .said
        .iter()
        .find(|l| l.starts_with("display "))
        .cloned()
        .unwrap_or_default();
    let window = format!(
        "{:.0},{:.0} {:.0}x{:.0} pt",
        s.frame.x, s.frame.y, s.frame.w, s.frame.h
    );
    let f = facts(
        kit,
        run,
        at,
        &gif,
        &e,
        &raw,
        format!(
            "in cargo's target directory, demo/{stem}.raw.mp4, not in the repository: it is the \
             whole display, every application started after the recorder in it"
        ),
        display,
        window,
        &c,
        s.said,
        s.events,
    )?;
    let text = text(&f, run.names)?;
    fs::copy(&raw, &keep).map_err(|e| format!("{}: {e}", keep.display()))?;
    let prov = keep_gif(&gif, &kit.out, &text, gate)?;
    let said = provenance::verify(&kit.out, run.root)?;
    println!(
        "gif:     {}, {} bytes, {} frames, {:.3} s, the film's {:.3} s",
        kit.out.display(),
        f.gif_bytes,
        e.frames,
        e.gif,
        e.film
    );
    println!("         its provenance beside it, {}", prov.display());
    for l in said {
        println!("         {l}");
    }
    println!("raw:     {}", keep.display());
    Ok(())
}

/// **The GIF kept, once `gate` has passed it where it was made**: `gif`
/// copied to `out` and `text` written beside it as its provenance file,
/// only after `gate` returns, so a refusal, an error or an interruption in
/// the gate leaves nothing at `out`. Returns the provenance file's path.
pub fn keep_gif(
    gif: &Path,
    out: &Path,
    text: &str,
    gate: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    gate(gif)?;
    fs::copy(gif, out).map_err(|e| format!("{}: {e}", out.display()))?;
    let prov = provenance::path_of(out);
    fs::write(&prov, text).map_err(|e| format!("{}: {e}", prov.display()))?;
    Ok(prov)
}

/// What the MP4 made: its frames, how long the film and it play.
pub struct Made {
    pub frames: usize,
    pub film: f64,
    pub mp4: f64,
}

/// **The film made the MP4**: ffmpeg crops it to `c`, samples it at
/// [`film::MP4_FPS`] and encodes it ([`film::mp4_args`]) into `mp4`, which
/// must run as long as the film plays ([`film::mp4_covers`]) and start at
/// its first frame.
pub fn encode_mp4(kit: &Kit, raw: &Path, c: &Crop, mp4: &Path) -> Result<Made, String> {
    run_ok(
        Command::new(&kit.ffmpeg).args(film::mp4_args(
            &raw.to_string_lossy(),
            c,
            &mp4.to_string_lossy(),
        )),
        "ffmpeg",
    )?;
    let plays = say(duration(kit).arg(raw), "ffprobe")?;
    let film = film::length(&plays)
        .ok_or_else(|| format!("ffprobe gave the film's length as {plays:?}"))?;
    let shown = say(duration(kit).arg(mp4), "ffprobe")?;
    let mp4_s = seconds(&shown, "the MP4's length")?;
    let mut probe = Command::new(&kit.ffprobe);
    probe.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-count_packets",
        "-show_entries",
        "stream=nb_read_packets,start_time",
        "-of",
        "csv=p=0",
    ]);
    let read = say(probe.arg(mp4), "ffprobe")?;
    let (start, count) = read
        .split_once(',')
        .ok_or_else(|| format!("ffprobe gave the MP4's stream as {read:?}"))?;
    let frames = count
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("ffprobe gave the MP4's frames as {count:?}"))?;
    if seconds(start, "the MP4's start")?.abs() > 0.001 {
        return Err(format!(
            "the MP4 starts at {start} s, not at the film's first frame"
        ));
    }
    film::mp4_covers(frames, film, mp4_s)?;
    Ok(Made {
        frames,
        film,
        mp4: mp4_s,
    })
}

/// **The take's timeline**: baud's journal and the typed commands, on the
/// film's clock, refused if it names this machine.
fn timeline_text(s: &Session, names: &[String]) -> Result<String, String> {
    let (first, read_ns) = film::film_start(&s.said)?;
    let read = s
        .started
        .ok_or("the line saying where the film starts was never read")?;
    let clock = timeline::Clock {
        read,
        host_ns: read_ns,
    };
    let journal = fs::read_to_string(&s.journal)
        .map_err(|e| format!("baud's take journal, {}: {e}", s.journal.display()))?;
    let mut rows = timeline::journal(&journal)?;
    rows.extend(
        s.scenes
            .iter()
            .map(|(t, command)| timeline::scene(clock.host(*t), command)),
    );
    let text = timeline::text(first, rows)?;
    let public = names::public_path(env!("CARGO_PKG_REPOSITORY"));
    let hit = names::named_outside(text.as_bytes(), names, &[&public]);
    if !hit.is_empty() {
        return Err(format!("the timeline would name this machine: {hit:?}"));
    }
    Ok(text)
}

/// **The MP4 and its timeline made, checked and written**: the film
/// cropped and encoded ([`encode_mp4`]), playing to the stop; the
/// repository still at the commit it was built from; the raw film kept in
/// cargo's target directory; then the MP4, `NAME.timeline.tsv` and
/// `NAME.mp4.provenance.toml` written beside each other.
fn finish_mp4(kit: &Kit, at: &Layout, run: &Run, target: &Path, s: Session) -> Result<(), String> {
    let raw = at.root.join("raw.mp4");
    let c = film::crop(&s.frame, &s.display)?;
    let mp4 = at.root.join(&kit.gif);
    println!(
        "encode:  the film cropped to baud's window ({})",
        c.filter()
    );
    let m = encode_mp4(kit, &raw, &c, &mp4)?;
    let stop = film::said_film(&s.said).ok_or("the recorder never said where its film stops")?;
    film::ends_at_stop(m.film, stop)?;
    let text = timeline_text(&s, run.names)?;
    let now = provenance::tree(run.root)?;
    if now.commit != run.tree.commit {
        return Err(format!(
            "the repository moved from {} to {} while it was filmed",
            run.tree.commit, now.commit
        ));
    }
    let stem = kit.gif.trim_end_matches(".mp4");
    let keep_dir = target.join("demo");
    fs::create_dir_all(&keep_dir).map_err(|e| format!("{}: {e}", keep_dir.display()))?;
    let keep = keep_dir.join(format!("{stem}.raw.mp4"));
    fs::copy(&raw, &keep).map_err(|e| format!("{}: {e}", keep.display()))?;
    fs::copy(&mp4, &kit.out).map_err(|e| format!("{}: {e}", kit.out.display()))?;
    let tsv = kit.out.with_file_name(format!("{stem}.timeline.tsv"));
    fs::write(&tsv, &text).map_err(|e| format!("{}: {e}", tsv.display()))?;
    let mut tools = run.tools.to_vec();
    tools.extend(kit.tools.iter().cloned());
    let tour = provenance::Tour {
        mp4: kit.gif.clone(),
        timeline: format!("{stem}.timeline.tsv"),
        command: run.command.clone(),
        from: run.from.clone(),
        tree: run.tree.clone(),
        macos: macos()?,
        mp4_sha256: provenance::sha256(&kit.out)?,
        mp4_bytes: bytes_of(&kit.out)?,
        frames: m.frames,
        fps: film::MP4_FPS,
        seconds: m.mp4,
        timeline_sha256: provenance::sha256(&tsv)?,
        nixpkgs: NIXPKGS.rsplit('/').next().unwrap_or(NIXPKGS).to_string(),
        tools,
        pins: run.pins.to_vec(),
        events: s.events.iter().map(|l| scrub(l, at)).collect(),
    };
    let credits = provenance::credit_line(provenance::LICENSES)?;
    let header = provenance::tour_header(&kit.gif);
    let header: Vec<&str> = header.iter().map(String::as_str).collect();
    let record = provenance::tour_document(&tour, credits).text(&header);
    let public = names::public_path(env!("CARGO_PKG_REPOSITORY"));
    let hit = names::named_outside(record.as_bytes(), run.names, &[&public]);
    if !hit.is_empty() {
        return Err(format!("the MP4's record would name this machine: {hit:?}"));
    }
    let prov = provenance::path_of(&kit.out);
    fs::write(&prov, &record).map_err(|e| format!("{}: {e}", prov.display()))?;
    println!(
        "mp4:     {}, {} bytes, {} frames at {}, {:.3} s, the film's {:.3} s",
        kit.out.display(),
        bytes_of(&kit.out)?,
        m.frames,
        film::MP4_FPS,
        m.mp4,
        m.film
    );
    println!(
        "         its timeline beside it, {}, {} rows",
        tsv.display(),
        text.lines().count().saturating_sub(1)
    );
    println!("         its record beside it, {}", prov.display());
    println!("video:   {}", provenance::end_card(&tour.tree));
    println!("raw:     {}", keep.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::Reading;

    /// A recorder that has already ended, successfully or not, saying one
    /// line.
    fn ended(program: &str) -> Filming {
        let child = Command::new(program)
            .stdin(Stdio::null())
            .spawn()
            .expect("the program");
        Filming {
            child,
            said: Arc::new(Mutex::new(vec!["film 1.000".to_string()])),
            started: Arc::new(Mutex::new(None)),
            err: Arc::new(Mutex::new(String::new())),
            threads: Vec::new(),
            display: Display {
                pt: (0.0, 0.0),
                scale: 0.0,
                px: (0, 0),
            },
        }
    }

    fn clear() -> Console {
        Console {
            registry_locked: Reading::No,
            session_locked: Reading::No,
            on_console: Reading::Yes,
            secure_input: Reading::No,
            display_asleep: Reading::No,
            display_active: Reading::Yes,
            saver: Reading::No,
        }
    }

    /// **A film that ends on a locked screen, a sleeping display or a
    /// session unread gives nothing to encode**: what the recorder said
    /// comes back only from a clear session, and `live` hands it on to the
    /// GIF only then. The check is read once the recorder has ended, not
    /// before. Perturbation: take the check out of `finish`, and the locked
    /// film returns its lines.
    #[test]
    fn a_film_ending_on_a_locked_screen_gives_no_gif() {
        let limit = Duration::from_secs(10);
        let said = ended("/usr/bin/true").finish(limit, clear).unwrap();
        assert_eq!(said, ["film 1.000"]);
        let locked = ended("/usr/bin/true")
            .finish(limit, || Console {
                registry_locked: Reading::Yes,
                ..clear()
            })
            .unwrap_err();
        assert!(
            locked.contains("the screen is locked") && locked.contains("no GIF was written"),
            "{locked}"
        );
        let asleep = ended("/usr/bin/true")
            .finish(limit, || Console {
                display_asleep: Reading::Yes,
                ..clear()
            })
            .unwrap_err();
        assert!(asleep.contains("asleep"), "{asleep}");
        let unread = ended("/usr/bin/true").finish(limit, || Console {
            session_locked: Reading::Unread,
            ..clear()
        });
        assert!(unread.is_err());
    }

    /// **The recorder's own refusal comes first**, and asks nothing of the
    /// session. The control that the check above is not the only way out.
    #[test]
    fn a_recorder_that_refused_is_named_for_it() {
        let said = ended("/usr/bin/false")
            .finish(Duration::from_secs(10), || {
                panic!("the session was read for a film the recorder refused")
            })
            .unwrap_err();
        assert!(said.contains("the recorder refused the film"), "{said}");
    }

    /// **A GIF the gate refuses leaves nothing in the output's directory**,
    /// and one it passes is kept with its provenance file; the gate is
    /// handed the GIF where it was made. Perturbation: copy the GIF before
    /// asking the gate and the refusal's assertion fails.
    #[test]
    fn a_refused_gif_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("demo-keep-gif-{}", std::process::id()));
        let (made, kept) = (dir.join("made"), dir.join("kept"));
        fs::create_dir_all(&made).unwrap();
        fs::create_dir_all(&kept).unwrap();
        let gif = made.join("trailer.gif");
        fs::write(&gif, b"GIF89a").unwrap();
        let out = kept.join("trailer.gif");
        let refuse = |g: &Path| -> Result<(), String> {
            assert_eq!(
                g,
                gif.as_path(),
                "the gate judges the GIF where it was made"
            );
            Err("flashes".to_string())
        };
        assert!(keep_gif(&gif, &out, "x", &refuse).is_err());
        assert_eq!(fs::read_dir(&kept).unwrap().count(), 0, "nothing kept");
        let pass = |_: &Path| Ok(());
        let prov = keep_gif(&gif, &out, "x", &pass).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"GIF89a");
        assert_eq!(fs::read_to_string(&prov).unwrap(), "x");
        fs::remove_dir_all(&dir).unwrap();
    }
}
