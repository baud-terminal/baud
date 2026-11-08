//! **The tour's two soundtracks**, `--sound`: one take's MP4 made twice
//! beside itself, `NAME.silent.mp4` with a track of silence and
//! `NAME.clicks.mp4` with a keyboard's clicks laid under every typed key
//! of its timeline (`timeline.rs`), from a Mechvibes sound pack's
//! directory, given as an argument and never kept here. The pack the
//! clicks version's credit names, and its files' SHA-256, are in
//! `assets/demo/LICENSES.md`, "Keyboard sounds"; another pack is refused
//! unless `--unpinned` says the credit does not cover it. There is no
//! music and no other effect.
//!
//! ```text
//! scripts/demo-sound.sh TOUR.mp4 TOUR.timeline.tsv PACK/
//! demo --sound FFMPEG_BIN TOUR.mp4 TOUR.timeline.tsv PACK/ [--unpinned]
//! ```
//!
//! **Which sound a row takes.** A timeline's key row is `key` or `enter`
//! and never says which key (`baud-app/src/take.rs` keeps no character),
//! so:
//! - `enter` plays the pack's `Enter`;
//! - `key` plays one of the pack's letter keys, `KeyA` to `KeyZ` as it
//!   defines them, drawn in turn from a generator with a fixed seed, so
//!   the same timeline makes the same draws: an arrow, a Backspace or a
//!   Ctrl-d sounds as a letter does;
//! - every other row plays nothing. A Command chord and a modifier
//!   pressed alone have no key row at all: baud's `Driver::typed`, where
//!   the journal takes its keys, is not reached by a chord the menu
//!   handles (Copy and Paste write only `copy` and `paste` rows, which
//!   are effects, not keys) nor by a modifier alone (`modifiers_changed`
//!   sends it on by itself). They are silent by construction.
//!
//! **What Mechvibes does, done here**, from its V2 config format
//! (`docs/soundpack-config-v2.md` at the pinned commit) and its audio
//! engine (`src/libs/audio/engine.rs`, `handle_key_event`,
//! `lookup_timing`, `play_segment`, `apply_fade`):
//! - a `"single"` pack: every key's `timing` is `[start_ms, end_ms]`
//!   slices of the one `audio_file`; one slice plays as the key goes
//!   down, a second as it comes up, and a key with more plays nothing;
//! - **a key the pack does not define plays nothing**: the engine plays a
//!   key only if `lookup_timing` finds it, with no other arm, so there is
//!   no default sound to fall back to (the format's guide says the same of
//!   a mouse code: the app "deliberately does not substitute another
//!   sound");
//! - a slice is cut at whole frames, `floor(ms / 1000 * rate)`, and faded
//!   in over 2 ms and out over 5 ms, each at most half the slice.
//!
//! The timeline holds no key's release, so a key's up slice follows its
//! down slice at once, as the pack's recording has them; that is the
//! original Mechvibes clip, one recording of a press and its release.
//!
//! **Where**: a row at `t_ms` starts its down slice at frame `t_ms * 48`
//! of the 48 kHz track, exactly; the track is as long as the MP4's video
//! stream, and a slice past its end is cut there. Slices are summed in
//! the timeline's order at [`GAIN`], and nothing is limited or clipped:
//! a track whose peak passes [`CEILING`] is turned down whole to it, so
//! the same inputs are the same bytes every run.
//! ffmpeg only decodes the pack's audio to 32-bit float at 48 kHz, and
//! encodes each track as AAC beside the MP4's video, copied, every
//! encoder told to be bit-exact.
//!
//! **Why this is Rust and not the ffmpeg graph the script used to
//! build**: an `adelay` stream a key, mixed by one `amix` of every row,
//! is thousands of inputs for the tour's keys, each padded with silence
//! up to its time; here a key is an add into one buffer, at an integer
//! frame, and the tests check the frames without nix or ffmpeg.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::provenance;
use crate::timeline;

/// The track's rate: frames a second.
pub const RATE: u32 = 48_000;
/// Interleaved channels, left then right.
pub const CHANNELS: usize = 2;
/// Mechvibes' fades of a slice, `FADE_IN_MS` and `FADE_OUT_MS` in its
/// `engine.rs`.
const FADE_IN_MS: f32 = 2.0;
const FADE_OUT_MS: f32 = 5.0;

/// **The pack the clicks version's credit names**: Mechvibes'
/// `soundpacks/keyboard/cherrymx-brown-abs` at tag v3.5.6, each file and
/// its SHA-256 (`assets/demo/LICENSES.md`).
pub const PACK_PIN: [(&str, &str); 2] = [
    (
        "config.json",
        "41385c5d6d635cf42c73b81c1257452eb63cb29d610e4f1035946dcfe912129b",
    ),
    (
        "sound.ogg",
        "1a77001c5741aff8102c967ba7b8f42d40771506f185174ff40d0de78a265385",
    ),
];

/// **Every slice's gain, -6 dB.** The pinned pack's `sound.ogg` itself
/// peaks at +0.1 dBFS (ffmpeg's `astats`), and its letter keys from 0.36
/// to 1.0, so at its `recommended_volume` of 1.0 one key reaches full
/// scale, and keys typed 40 ms apart, each slice about 170 ms long, sum
/// past it: a dry run of 36 keys at that pace peaked at 1.083 at unit
/// gain.
pub const GAIN: f32 = 0.5;

/// **The loudest a track may be, -1 dBFS**, left below full scale for
/// the AAC encoder's overshoot. Rows can share a time: `take.rs` stamps
/// every key sent before a read with the frame that draws it, so keys
/// whose echo came late, over ssh or in a program that draws nothing,
/// land together, and their slices add. A dry run of five minutes with
/// groups of two to four such rows peaked at 1.292 at [`GAIN`]; a track
/// past this is turned down whole to it ([`level`]), not clipped.
pub const CEILING: f32 = 0.89;

/// The generator's seed for the letters `key` rows draw: "baud".
const SEED: u32 = 0x6261_7564;

/// **A Mechvibes pack's config read**: its audio file and every key's
/// slices, `[start_ms, end_ms]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub audio_file: String,
    pub keys: BTreeMap<String, Vec<[f64; 2]>>,
}

/// One key's sound laid: the frame its down slice starts at and the key
/// it plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click {
    pub at: usize,
    pub key: String,
}

/// **What a timeline lays**: the clicks, and the key rows that play
/// nothing because the pack defines no key for them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub clicks: Vec<Click>,
    pub undefined: usize,
}

/// **The pack's `config.json`, a V2 `"single"` pack.** A `"multi"` pack
/// and a V1 config are refused by name, since neither is read here.
pub fn pack(config: &str) -> Result<Pack, String> {
    let v = Json::parse(config)?;
    if v.get("defines").is_some() || v.get("key_define_type").is_some() {
        return Err("the pack's config.json is Mechvibes' V1 format; this reads V2".to_string());
    }
    match v.get("definition_method").and_then(Json::str) {
        Some("single") => {}
        Some(m) => return Err(format!("the pack is {m:?}; only a \"single\" pack is read")),
        None => return Err("the pack's config.json has no definition_method".to_string()),
    }
    let audio_file = v
        .get("audio_file")
        .and_then(Json::str)
        .ok_or("the pack's config.json has no audio_file")?
        .to_string();
    let defs = v
        .get("definitions")
        .or_else(|| v.get("defs"))
        .and_then(Json::obj)
        .ok_or("the pack's config.json has no definitions")?;
    let mut keys = BTreeMap::new();
    for (name, def) in defs {
        let timing = def
            .get("timing")
            .and_then(Json::arr)
            .ok_or_else(|| format!("the pack's {name} has no timing"))?;
        let mut slices = Vec::new();
        for s in timing {
            match s.arr() {
                Some([Json::Num(a), Json::Num(b)]) => slices.push([*a, *b]),
                _ => {
                    return Err(format!(
                        "the pack's {name} has a timing that is not [start, end]"
                    ));
                }
            }
        }
        keys.insert(name.clone(), slices);
    }
    Ok(Pack { audio_file, keys })
}

/// **The timeline's key rows, as clicks**: [`timeline::HEADER`] first,
/// then `enter` as the pack's `Enter`, `key` as a letter drawn, and every
/// other row nothing.
pub fn plan(timeline_text: &str, pack: &Pack) -> Result<Plan, String> {
    let mut lines = timeline_text.lines();
    if lines.next() != Some(timeline::HEADER) {
        return Err("the timeline does not start with the timeline's header".to_string());
    }
    let letters: Vec<String> = ('A'..='Z')
        .map(|c| format!("Key{c}"))
        .filter(|k| pack.keys.contains_key(k))
        .collect();
    let mut state = SEED;
    let mut plan = Plan::default();
    for (n, line) in lines.enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 7 {
            return Err(format!("line {} of the timeline is not a row", n + 2));
        }
        let key = match f[1] {
            "enter" => "Enter".to_string(),
            "key" => {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match letters.len() {
                    0 => String::new(),
                    len => letters[(state >> 16) as usize % len].clone(),
                }
            }
            _ => continue,
        };
        let ms: u64 = f[0]
            .parse()
            .map_err(|_| format!("line {} of the timeline has no time", n + 2))?;
        match pack.keys.get(&key).is_some_and(|s| sounds(s)) {
            true => plan.clicks.push(Click {
                at: (ms * u64::from(RATE / 1000)) as usize,
                key,
            }),
            false => plan.undefined += 1,
        }
    }
    Ok(plan)
}

/// Whether a key's slices play, as Mechvibes' `lookup_timing` has it:
/// one or two.
fn sounds(slices: &[[f64; 2]]) -> bool {
    matches!(slices.len(), 1 | 2)
}

/// **The frame a time in the pack's audio falls on**: whole frames first,
/// as Mechvibes' `play_segment` cuts.
fn frame_at(ms: f64) -> usize {
    (ms / 1000.0 * f64::from(RATE)) as usize
}

/// **The silent track**: `frames` frames of zeros.
pub fn silence(frames: usize) -> Vec<f32> {
    vec![0.0; frames * CHANNELS]
}

/// **The clicks track**: `frames` frames, each click's down slice of
/// `audio` (interleaved stereo at [`RATE`]) added at its frame, its up
/// slice straight after, each faded as Mechvibes fades a slice.
pub fn lay(audio: &[f32], pack: &Pack, plan: &Plan, frames: usize) -> Vec<f32> {
    let mut out = silence(frames);
    let have = audio.len() / CHANNELS;
    for c in &plan.clicks {
        let Some(slices) = pack.keys.get(&c.key) else {
            continue;
        };
        let mut at = c.at;
        for [start, end] in slices {
            let (a, b) = (frame_at(*start), frame_at(*end).min(have));
            if *start < 0.0 || b <= a {
                continue;
            }
            let mut slice = audio[a * CHANNELS..b * CHANNELS].to_vec();
            fade(&mut slice);
            for (k, s) in slice.iter().enumerate() {
                match out.get_mut(at * CHANNELS + k) {
                    Some(o) => *o += s * GAIN,
                    None => break,
                }
            }
            at += b - a;
        }
    }
    out
}

/// **The track kept under [`CEILING`]**: one whose peak passes it is
/// scaled whole so that its peak is the ceiling, every sample by the
/// same factor, which the same track always gives; a track at or under
/// it is left. Returns the peak and the factor.
pub fn level(track: &mut [f32]) -> (f32, f32) {
    let peak = track.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    if peak <= CEILING {
        return (peak, 1.0);
    }
    let k = CEILING / peak;
    for s in track.iter_mut() {
        *s *= k;
    }
    (peak, k)
}

/// Mechvibes' `apply_fade`: a linear ramp in over 2 ms and out over 5 ms,
/// each at most half the slice, every channel of a frame alike.
fn fade(slice: &mut [f32]) {
    let n = slice.len() / CHANNELS;
    let ramp = |ms: f32| (((ms / 1000.0) * RATE as f32) as usize).min(n / 2);
    let (fin, fout) = (ramp(FADE_IN_MS), ramp(FADE_OUT_MS));
    for f in 0..fin {
        let g = f as f32 / fin as f32;
        for s in &mut slice[f * CHANNELS..(f + 1) * CHANNELS] {
            *s *= g;
        }
    }
    for f in 0..fout {
        let g = f as f32 / fout as f32;
        let i = n - 1 - f;
        for s in &mut slice[i * CHANNELS..(i + 1) * CHANNELS] {
            *s *= g;
        }
    }
}

/// **What a command line runs**: the soundtracks, or the rest of the
/// demo, whose session opens baud's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `--sound` anywhere: [`main`] alone, which opens no window.
    Sound,
    /// Anything else: the demo's own `main`, the session among it.
    Run,
}

/// **The mode of `args`**, the command line less the program: every line
/// with `--sound` anywhere in it is [`Mode::Sound`], a bad one too, and no
/// other line is, so a line that also names the session's flags is never
/// the session.
pub fn mode(args: &[String]) -> Mode {
    match args.iter().any(|a| a == "--sound") {
        true => Mode::Sound,
        false => Mode::Run,
    }
}

/// The sound command's one form, which [`job`] takes and nothing else.
pub const USAGE: &str =
    "usage: demo --sound FFMPEG_BIN TOUR.mp4 TOUR.timeline.tsv PACK/ [--unpinned]";

/// **`args` read as [`USAGE`]'s form**, exactly: `--sound` first, four
/// arguments none of which is a flag, then `--unpinned` or nothing. Any
/// other line is `None`, one that adds a flag of the session's among them
/// (`--dry`, `--record`), so a mixed line does neither.
fn job(args: &[String]) -> Option<Job> {
    let (pinned, four) = match args {
        [s, four @ .., u] if s == "--sound" && u == "--unpinned" => (false, four),
        [s, four @ ..] if s == "--sound" => (true, four),
        _ => return None,
    };
    match four {
        [bin, tour, tsv, dir] if four.iter().all(|a| !a.starts_with('-')) => Some(Job {
            bin: PathBuf::from(bin),
            tour: PathBuf::from(tour),
            timeline: PathBuf::from(tsv),
            pack: PathBuf::from(dir),
            pinned,
        }),
        _ => None,
    }
}

/// **`--sound`**, [`Mode::Sound`]'s whole run: both versions made, or why
/// not, and the exit code: 2 and [`USAGE`] for a line not of its form,
/// before anything is read or run.
pub fn main(args: &[String]) -> ExitCode {
    let Some(job) = job(args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(&job) {
        Ok(said) => {
            println!("{said}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("demo --sound: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, PartialEq)]
struct Job {
    bin: PathBuf,
    tour: PathBuf,
    timeline: PathBuf,
    pack: PathBuf,
    pinned: bool,
}

fn run(j: &Job) -> Result<String, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let text = read(&j.timeline)?;
    let config = j.pack.join("config.json");
    let pack = pack(&read(&config)?)?;
    let audio_path = j.pack.join(&pack.audio_file);
    if j.pinned {
        for (file, want) in PACK_PIN {
            let p = j.pack.join(file);
            let got = provenance::sha256(&p)?;
            if got != want {
                return Err(format!(
                    "{} is not the pinned pack's (sha256 {got}, pinned {want}); \
                     the clicks credit does not cover it, so --unpinned is needed",
                    p.display()
                ));
            }
        }
        if pack.audio_file != PACK_PIN[1].0 {
            return Err(format!(
                "the pinned pack plays {}, not {}",
                PACK_PIN[1].0, pack.audio_file
            ));
        }
    }
    let plan = plan(&text, &pack)?;
    let seconds: f64 = ffprobe(
        j,
        &["-select_streams", "v:0", "-show_entries", "stream=duration"],
    )?
    .trim()
    .parse()
    .map_err(|_| format!("{} has no video stream's duration", j.tour.display()))?;
    let frames = (seconds * f64::from(RATE)).round() as usize;
    let audio = decode(j, &audio_path)?;
    let mut clicks = lay(&audio, &pack, &plan, frames);
    let (peak, turned) = level(&mut clicks);
    let stem = j
        .tour
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("the MP4 has no name")?;
    let dir = j.tour.parent().unwrap_or(Path::new("."));
    let work = dir.join(format!(".{stem}.sound.{}", std::process::id()));
    std::fs::create_dir(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let made = (|| {
        for (version, track) in [("silent", silence(frames)), ("clicks", clicks)] {
            let out = dir.join(format!("{stem}.{version}.mp4"));
            mux(j, &work, version, &track, &out)?;
        }
        Ok::<(), String>(())
    })();
    let _ = std::fs::remove_dir_all(&work);
    made?;
    Ok(format!(
        "demo --sound: {stem}.silent.mp4 and {stem}.clicks.mp4 beside {}, {frames} frames at {RATE} Hz; \
         {} keys clicked, {} the pack defines no key for and so silent, the clicks' peak {peak:.3} at a gain of {GAIN}, scaled by {turned:.3}{}",
        j.tour.display(),
        plan.clicks.len(),
        plan.undefined,
        if j.pinned {
            ""
        } else {
            "; an unpinned pack, which the clicks credit does not cover"
        }
    ))
}

fn ffprobe(j: &Job, args: &[&str]) -> Result<String, String> {
    let out = Command::new(j.bin.join("ffprobe"))
        .args(["-v", "error"])
        .args(args)
        .args(["-of", "csv=p=0", "--"])
        .arg(&j.tour)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("ffprobe: {e}"))?;
    if !out.status.success() {
        return Err(format!("ffprobe could not read {}", j.tour.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The pack's audio as interleaved stereo 32-bit float at [`RATE`].
fn decode(j: &Job, audio: &Path) -> Result<Vec<f32>, String> {
    let out = Command::new(j.bin.join("ffmpeg"))
        .args(["-hide_banner", "-v", "error", "-nostdin", "-i"])
        .arg(audio)
        .args(["-map", "0:a:0", "-f", "f32le", "-ac", "2", "-ar"])
        .arg(RATE.to_string())
        .args(["-threads", "1", "-"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !out.status.success() {
        return Err(format!("ffmpeg could not decode {}", audio.display()));
    }
    Ok(out
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// `track` encoded as AAC beside the MP4's video, copied, into `out`.
fn mux(j: &Job, work: &Path, version: &str, track: &[f32], out: &Path) -> Result<(), String> {
    let raw = work.join(format!("{version}.f32"));
    let bytes: Vec<u8> = track.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(&raw, bytes).map_err(|e| format!("{}: {e}", raw.display()))?;
    let made = work.join(format!("{version}.mp4"));
    let status = Command::new(j.bin.join("ffmpeg"))
        .args(["-hide_banner", "-v", "error", "-nostdin", "-y"])
        .args(["-f", "f32le", "-ar"])
        .arg(RATE.to_string())
        .args(["-ac", "2", "-i"])
        .arg(&raw)
        .arg("-i")
        .arg(&j.tour)
        .args(["-map", "1:v:0", "-map", "0:a:0", "-c:v", "copy"])
        .args(["-c:a", "aac", "-b:a", "192k", "-threads", "1"])
        .args(["-fflags", "+bitexact", "-flags:a", "+bitexact"])
        .args(["-map_metadata", "-1", "-map_chapters", "-1"])
        .args(["-movflags", "+faststart", "-f", "mp4", "--"])
        .arg(&made)
        .status()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !status.success() {
        return Err(format!("ffmpeg could not make the {version} version"));
    }
    std::fs::rename(&made, out).map_err(|e| format!("{}: {e}", out.display()))
}

/// **The JSON a pack's config is**, read whole: no number is lost to a
/// type, and an object keeps its keys' order.
#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
        };
        let v = p.value()?;
        p.space();
        match p.i == p.s.len() {
            true => Ok(v),
            false => Err(p.err("text after the value")),
        }
    }
    fn get(&self, key: &str) -> Option<&Json> {
        self.obj()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    fn obj(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Obj(o) => Some(o),
            _ => None,
        }
    }
    fn arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("the pack's config.json, byte {}: {what}", self.i)
    }
    fn space(&mut self) {
        while self.s.get(self.i).is_some_and(|b| b" \t\r\n".contains(b)) {
            self.i += 1;
        }
    }
    fn eat(&mut self, b: u8) -> bool {
        self.space();
        let hit = self.s.get(self.i) == Some(&b);
        if hit {
            self.i += 1;
        }
        hit
    }
    fn word(&mut self, w: &str, v: Json) -> Result<Json, String> {
        match self.s[self.i..].starts_with(w.as_bytes()) {
            true => {
                self.i += w.len();
                Ok(v)
            }
            false => Err(self.err("not a value")),
        }
    }
    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.s.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut o = Vec::new();
                if self.eat(b'}') {
                    return Ok(Json::Obj(o));
                }
                loop {
                    self.space();
                    let k = self.string()?;
                    if !self.eat(b':') {
                        return Err(self.err("no ':' after a key"));
                    }
                    o.push((k, self.value()?));
                    if self.eat(b'}') {
                        return Ok(Json::Obj(o));
                    }
                    if !self.eat(b',') {
                        return Err(self.err("no ',' or '}' in an object"));
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                if self.eat(b']') {
                    return Ok(Json::Arr(a));
                }
                loop {
                    a.push(self.value()?);
                    if self.eat(b']') {
                        return Ok(Json::Arr(a));
                    }
                    if !self.eat(b',') {
                        return Err(self.err("no ',' or ']' in an array"));
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let at = self.i;
                while self
                    .s
                    .get(self.i)
                    .is_some_and(|b| b"+-.eE0123456789".contains(b))
                {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[at..self.i])
                    .ok()
                    .and_then(|t| t.parse::<f64>().ok())
                    .map(Json::Num)
                    .ok_or_else(|| self.err("not a number"))
            }
            _ => Err(self.err("not a value")),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        if self.s.get(self.i) != Some(&b'"') {
            return Err(self.err("not a string"));
        }
        self.i += 1;
        let mut out = Vec::new();
        loop {
            match self.s.get(self.i) {
                None => return Err(self.err("an unclosed string")),
                Some(b'"') => {
                    self.i += 1;
                    return String::from_utf8(out).map_err(|_| self.err("a string not UTF-8"));
                }
                Some(b'\\') => {
                    let e = *self
                        .s
                        .get(self.i + 1)
                        .ok_or_else(|| self.err("an escape cut off"))?;
                    self.i += 2;
                    let c = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hex = self
                                .s
                                .get(self.i..self.i + 4)
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .ok_or_else(|| self.err("a \\u escape not four hex digits"))?;
                            self.i += 4;
                            char::from_u32(hex).unwrap_or('\u{fffd}')
                        }
                        _ => return Err(self.err("an unknown escape")),
                    };
                    let mut b = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
                }
                Some(&b) => {
                    out.push(b);
                    self.i += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pack of sine blips, made here: each key's down slice a 1 kHz
    /// blip of 20 ms and its up slice one of 10 ms, every slice 40 ms
    /// apart in a 48 kHz stereo file that is silent between them.
    fn blips(keys: &[&str]) -> (Pack, Vec<f32>) {
        let mut audio = Vec::new();
        let mut defs = BTreeMap::new();
        for (n, k) in keys.iter().enumerate() {
            let base = n as f64 * 80.0;
            defs.insert(
                k.to_string(),
                vec![[base, base + 20.0], [base + 40.0, base + 50.0]],
            );
        }
        let frames = keys.len() * 80 * 48;
        for f in 0..frames {
            let ms = (f % (80 * 48)) as f64 / 48.0;
            let on = ms < 20.0 || (40.0..50.0).contains(&ms);
            let s = match on {
                true => {
                    (2.0 * std::f64::consts::PI * 1000.0 * f as f64 / 48_000.0).sin() as f32 * 0.5
                }
                false => 0.0,
            };
            audio.extend([s, s]);
        }
        (
            Pack {
                audio_file: "blips.wav".to_string(),
                keys: defs,
            },
            audio,
        )
    }

    fn rows(rows: &[(u64, &str)]) -> String {
        let mut t = format!("{}\n", timeline::HEADER);
        for (ms, ev) in rows {
            t.push_str(&format!("{ms}\t{ev}\t-\t10\t20\t\t\n"));
        }
        t
    }

    /// The first frame at or after `from` whose left sample is not zero.
    fn onset(track: &[f32], from: usize) -> Option<usize> {
        (from..track.len() / CHANNELS).find(|&f| track[f * CHANNELS] != 0.0)
    }

    /// **Five keys at known times click at exactly those frames, to one
    /// frame at 48 kHz**: a synthetic timeline of four `key` rows and an
    /// `enter`, with a copy and a trail between them, over a pack of sine
    /// blips; each click's first sound is within one frame of `t_ms * 48`
    /// (the fade's first frame is a zero, so it lands one after), and
    /// the frames before each are silent. The copy and the trail lay
    /// nothing. Perturbation: lay a click at `t_ms * 44`, or one frame
    /// late past the fade, and the offsets miss.
    #[test]
    fn five_keys_click_at_their_frames_to_one_sample() {
        let (pack, audio) = blips(&["Enter", "KeyA", "KeyB", "KeyC"]);
        let times = [120_u64, 400, 731, 1005, 1499];
        let t = rows(&[
            (times[0], "key"),
            (times[1], "key"),
            (500, "copy"),
            (times[2], "key"),
            (900, "trail"),
            (times[3], "key"),
            (times[4], "enter"),
        ]);
        let plan = plan(&t, &pack).unwrap();
        assert_eq!(plan.clicks.len(), 5, "{plan:?}");
        assert_eq!(plan.undefined, 0);
        assert_eq!(plan.clicks[4].key, "Enter");
        let frames = 2 * RATE as usize;
        let track = lay(&audio, &pack, &plan, frames);
        assert_eq!(track.len(), frames * CHANNELS);
        let mut from = 0;
        for (ms, c) in times.iter().zip(&plan.clicks) {
            let want = (*ms * 48) as usize;
            assert_eq!(c.at, want);
            let got = onset(&track, from).expect("a click");
            assert!(
                got.abs_diff(want) <= 1,
                "the click for {ms} ms starts at frame {got}, not {want}"
            );
            // Each down slice is 20 ms and its up 10 ms; the next row is
            // at least 270 ms on, so start looking past this click.
            from = want + 31 * 48;
        }
        assert!(
            track[..times[0] as usize * 48 * CHANNELS]
                .iter()
                .all(|s| *s == 0.0)
        );
    }

    /// **A key the pack does not define plays nothing, as Mechvibes plays
    /// nothing for it**: a pack with letters and no `Enter` lays the keys
    /// and leaves the Return's time silent, and counts it. Mechvibes'
    /// engine looks a key up and, finding none, does not play: Mechvibes
    /// has no default sound to fall back to. The control: with `Enter`
    /// defined the same Return sounds. Perturbation: let an undefined key
    /// fall back to a letter's sound, and the Return's frames are not
    /// silent.
    #[test]
    fn a_key_the_pack_does_not_define_plays_nothing() {
        let t = rows(&[(100, "key"), (600, "enter"), (1100, "key")]);
        let (pack, audio) = blips(&["KeyA", "KeyB"]);
        let without = plan(&t, &pack).unwrap();
        assert_eq!(
            (without.clicks.len(), without.undefined),
            (2, 1),
            "{without:?}"
        );
        let track = lay(&audio, &pack, &without, RATE as usize * 2);
        let enter = 600 * 48 * CHANNELS..1100 * 48 * CHANNELS;
        assert!(
            track[enter.clone()].iter().all(|s| *s == 0.0),
            "the Return sounded"
        );
        assert!(
            track[..enter.start].iter().any(|s| *s != 0.0),
            "the key before it"
        );

        let (with, audio) = blips(&["Enter", "KeyA", "KeyB"]);
        let control = plan(&t, &with).unwrap();
        assert_eq!((control.clicks.len(), control.undefined), (3, 0));
        let track = lay(&audio, &with, &control, RATE as usize * 2);
        assert!(
            track[enter].iter().any(|s| *s != 0.0),
            "the control: Enter defined"
        );
    }

    /// **The silent version is all zeros for the whole MP4**: as many
    /// frames as the video is long, stereo, every sample zero, however
    /// many keys the timeline has. Perturbation: make one of its samples
    /// 1e-30, or the track a frame short, and an assertion fails.
    #[test]
    fn the_silent_track_is_all_zeros_for_its_length() {
        let frames = 3 * RATE as usize + 17;
        let track = silence(frames);
        assert_eq!(track.len(), frames * CHANNELS);
        assert!(track.iter().all(|s| s.to_bits() == 0));
        // The control: the clicks track of the same length is not.
        let (pack, audio) = blips(&["KeyA"]);
        let plan = plan(&rows(&[(10, "key")]), &pack).unwrap();
        assert!(lay(&audio, &pack, &plan, frames).iter().any(|s| *s != 0.0));
    }

    /// **The same timeline and pack lay the same bytes, and the letters
    /// are drawn, not one key**: two plans of one timeline are equal; a
    /// timeline of forty keys draws more than one letter; a key past the
    /// MP4's end is cut there, not wrapped. Perturbation: seed the
    /// generator from the clock, and the two plans differ.
    #[test]
    fn the_same_inputs_lay_the_same_track() {
        let (pack, audio) = blips(&["Enter", "KeyA", "KeyB", "KeyC", "KeyD"]);
        let t = rows(&(0..40).map(|n| (n * 40, "key")).collect::<Vec<_>>());
        let (a, b) = (plan(&t, &pack).unwrap(), plan(&t, &pack).unwrap());
        assert_eq!(a, b);
        let mut keys: Vec<&str> = a.clicks.iter().map(|c| c.key.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert!(keys.len() > 1, "{keys:?}");
        let short = 100 * 48;
        let one = lay(&audio, &pack, &a, short);
        assert_eq!(one, lay(&audio, &pack, &b, short));
        assert_eq!(one.len(), short * CHANNELS);
    }

    /// **Each slice is laid at [`GAIN`], and a track past [`CEILING`] is
    /// turned down whole, not clipped**: a pack whose audio is a constant
    /// 1.0 lays one key at exactly `GAIN` once its fade-in is done, which
    /// [`level`] leaves; three keys at one time sum to 1.5, which it
    /// scales by `CEILING / 1.5` everywhere, so the sum is the ceiling and
    /// a lone key elsewhere in the track keeps its share of it, not
    /// clipped to it. Perturbation: lay at unit gain, and the one key's
    /// sample is 1.0; or clamp each sample to the ceiling in place of the
    /// scale, and the lone key stays at 0.5.
    #[test]
    fn slices_are_laid_at_the_gain_and_a_loud_track_is_turned_down() {
        let pack = Pack {
            audio_file: "flat.wav".to_string(),
            keys: BTreeMap::from([("KeyA".to_string(), vec![[0.0, 50.0]])]),
        };
        let audio = vec![1.0_f32; 50 * 48 * CHANNELS];
        let mid = |ms: usize| (ms * 48 + 200) * CHANNELS;
        let mut one = lay(
            &audio,
            &pack,
            &plan(&rows(&[(100, "key")]), &pack).unwrap(),
            RATE as usize,
        );
        assert_eq!(one[mid(100)], GAIN);
        assert_eq!(level(&mut one), (GAIN, 1.0));
        assert_eq!(one[mid(100)], GAIN);
        let t = rows(&[(100, "key"), (100, "key"), (100, "key"), (500, "key")]);
        let mut loud = lay(&audio, &pack, &plan(&t, &pack).unwrap(), RATE as usize);
        assert_eq!(loud[mid(100)], 3.0 * GAIN);
        let (peak, k) = level(&mut loud);
        assert_eq!((peak, k), (1.5, CEILING / 1.5));
        assert_eq!(loud[mid(100)], 1.5 * k);
        assert_eq!(loud[mid(500)], GAIN * k);
        assert!(loud.iter().all(|s| s.abs() <= CEILING));
    }

    /// **`--sound` never reaches the session, so it opens no window and
    /// plays nothing**, shown without running the demo:
    /// - [`mode`] makes every line with `--sound` anywhere in it
    ///   [`Mode::Sound`], `--dry --sound` and `--record x --sound` among
    ///   them, and no other line; and [`main`] answers 2 for any of them
    ///   not of [`USAGE`]'s exact form, before anything is read or run;
    /// - each `main` in `demo.rs` is the one `match` on it, whose `Sound`
    ///   arm is [`main`] and returns what it returns, and `run::main`, the
    ///   session that opens baud's window, is named in `demo.rs` on one
    ///   line only, the macOS `Run` arm;
    /// - this file, its tests aside, starts no process but ffprobe and
    ///   ffmpeg (and `shasum`, through `provenance::sha256`), and names none
    ///   of the modules or APIs that open a window or play a sound.
    ///
    /// The controls: the source probes find the `Run` arm and this file's
    /// spawns. Perturbation: map `--sound` to `Mode::Run`, or read it
    /// only as the first argument, or let `job` take a flag among the
    /// four, or call `run::main()` in the `Sound` arm, and an assertion
    /// fails; no break is ever run as the demo, only as this test.
    #[test]
    fn sound_never_reaches_the_session() {
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        // A line with `--sound` anywhere is the mixer's, and one that is not
        // its exact form is refused there with 2, before anything is read
        // or run: `job` is pure, and `main` returns before `run` for it.
        for bad in [
            &["--sound"][..],
            &["--sound", "a", "b"],
            &["--dry", "--sound"],
            &["--record", "x", "--sound"],
            &["--sound", "--dry"],
            &["--sound", "bin", "t.mp4", "t.tsv", "pack/", "--dry"],
            &["--sound", "bin", "t.mp4", "--dry", "pack/"],
            &[
                "--sound",
                "bin",
                "t.mp4",
                "t.tsv",
                "pack/",
                "--unpinned",
                "x",
            ],
            &["--sound", "--unpinned", "bin", "t.mp4", "t.tsv", "pack/"],
        ] {
            assert_eq!(mode(&s(bad)), Mode::Sound, "{bad:?}");
            assert_eq!(job(&s(bad)), None, "{bad:?}");
            assert_eq!(main(&s(bad)), ExitCode::from(2), "{bad:?}");
        }
        let good = ["--sound", "bin", "t.mp4", "t.tsv", "pack/"];
        let want = Job {
            bin: PathBuf::from("bin"),
            tour: PathBuf::from("t.mp4"),
            timeline: PathBuf::from("t.tsv"),
            pack: PathBuf::from("pack/"),
            pinned: true,
        };
        assert_eq!(mode(&s(&good)), Mode::Sound);
        assert_eq!(job(&s(&good)), Some(want));
        let loose = [&good[..], &["--unpinned"]].concat();
        let got = job(&s(&loose)).expect("the form with --unpinned");
        assert!(!got.pinned && got.pack == Path::new("pack/"));
        assert!(
            USAGE.contains("demo --sound FFMPEG_BIN TOUR.mp4 TOUR.timeline.tsv PACK/ [--unpinned]")
        );
        for other in [
            &[][..],
            &["--record", "tour.mp4"],
            &["--dry"],
            &["--verify", "trailer.gif"],
        ] {
            assert_eq!(mode(&s(other)), Mode::Run, "{other:?}");
        }
        let demo = include_str!("../demo.rs");
        let mains: Vec<&str> = demo
            .split("fn main() -> std::process::ExitCode {\n")
            .skip(1)
            .collect();
        assert_eq!(mains.len(), 2, "the macOS main and the other");
        for (m, run) in mains.iter().zip(["run::main()", "other(&args)"]) {
            let want = format!(
                "    let args: Vec<String> = std::env::args().skip(1).collect();\n    \
                 match sound::mode(&args) {{\n        \
                 sound::Mode::Sound => sound::main(&args),\n        \
                 sound::Mode::Run => {run},\n    }}\n}}\n"
            );
            assert!(
                m.starts_with(&want),
                "a main is not the match:\n{}",
                &m[..m.len().min(300)]
            );
        }
        let session: Vec<&str> = demo.lines().filter(|l| l.contains("run::main")).collect();
        assert_eq!(session, ["        sound::Mode::Run => run::main(),"]);
        let me = include_str!("sound.rs");
        let code = &me[..me.find("#[cfg(test)]").expect("the tests")];
        let spawned: Vec<&str> = code
            .split("Command::new(")
            .skip(1)
            .map(|c| &c[..c.find(')').unwrap() + 1])
            .collect();
        assert_eq!(
            spawned,
            [
                "j.bin.join(\"ffprobe\")",
                "j.bin.join(\"ffmpeg\")",
                "j.bin.join(\"ffmpeg\")"
            ]
        );
        for banned in [
            "mac::",
            "run::",
            "session::",
            "record::",
            "film::",
            "objc2",
            "NSWindow",
            "afplay",
            "\"open\"",
            "\"say\"",
            "BAUD_",
        ] {
            assert!(!code.contains(banned), "sound.rs names {banned}");
        }
        assert!(
            me.contains("run::"),
            "the control: the probe finds what is there"
        );
    }

    /// **The pinned pack's config reads, and the formats it cannot read
    /// are refused by name**: a V2 `"single"` config of the pinned pack's
    /// shape gives its audio file and each key's two slices; a V1 config
    /// (`defines`), a `"multi"` pack and a timing that is not
    /// `[start, end]` are refused. Perturbation: read `timing`'s slices
    /// as `[start, length]`, and Enter's end is wrong.
    #[test]
    fn a_v2_single_config_reads_and_the_rest_is_refused() {
        let v2 = r#"{
            "audio_file": "sound.ogg", "config_version": "2",
            "definition_method": "single", "author": "Mechvibes",
            "definitions": {
                "Enter": { "timing": [[26548.0, 26607.5], [26607.5, 26667.0]] },
                "KeyA": { "timing": [[22324.0, 22407.5]] }
            },
            "name": "CherryMX Brown - ABS keycaps",
            "options": { "random_pitch": false, "recommended_volume": 1.0 },
            "tags": ["pre-installed"], "x": null, "e": "é\n"
        }"#;
        let p = pack(v2).unwrap();
        assert_eq!(p.audio_file, "sound.ogg");
        assert_eq!(
            p.keys["Enter"],
            vec![[26548.0, 26607.5], [26607.5, 26667.0]]
        );
        assert_eq!(p.keys["KeyA"].len(), 1);
        let v1 = r#"{"key_define_type": "single", "sound": "sound.ogg", "defines": {"28": [26548, 119]}}"#;
        assert!(pack(v1).unwrap_err().contains("V1"));
        let multi = v2.replace("\"single\"", "\"multi\"");
        assert!(pack(&multi).unwrap_err().contains("multi"));
        let bad = v2.replace("[22324.0, 22407.5]", "[22324.0]");
        assert!(pack(&bad).is_err());
        assert!(pack("{\"a\": 1} x").is_err());
    }
}
