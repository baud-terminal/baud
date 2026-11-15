//! **The README's GIF, cut from the headless session's film**: the tour
//! played without a window (`--headless DIR`, `film::session` in
//! `baud-app/src/`) writes every frame, the window's title strip composed
//! over it, into a lossless film, `DIR/session.mkv`, 100 frames a second,
//! and beside it each scene's frames by the tour's own marks
//! (`session.mkv.cut`) and each frame's text as drawn (`session.mkv.text`).
//! This cuts [`SCENES`] out of the film by those marks, samples them at
//! [`provenance::FPS`] frames a second with the pinned ffmpeg and encodes
//! them with the pinned gifski at [`provenance::GIFSKI`]'s settings exactly
//! -- the trailer's, 1164 pixels wide -- into `DIR/session.gif`, then
//! decodes the GIF with baud's own decoder and holds its frames, length and
//! size to the cut ([`decoded`], `film::covers`). Beside it goes
//! `session.gif.provenance.toml` ([`document`]), which `--verify` reads as
//! it reads the trailer's, and which names nothing of this machine.
//!
//! Nothing here opens a window or films the screen: the frames are the
//! renderer's, drawn by baud's test.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::film;
use crate::names;
use crate::provenance::{self, Doc, Tree};

/// The GIF's name, beside the session's film: not the old README clip's
/// and not the trailer's, whose names belong to other takes.
pub const GIF: &str = "session.gif";

/// **The scenes the GIF plays**, by the tour's marks (`session::SESSION`):
/// the whole tour, one stretch, from baud's config in helix to the closing
/// line.
pub const SCENES: [u8; 16] = [1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];

/// **The first line of the pinned ffmpeg's `-version`**: ffmpeg-headless
/// at [`crate::run::NIXPKGS`], the logo films' encoder, which
/// `baud-pictures` checks the same way (`tools/baud-pictures/src/logo.rs`,
/// `FFMPEG`).
pub const FFMPEG: &str = "ffmpeg version 9.0.1";

/// **The pinned programs**: ffmpeg and gifski from [`crate::run::NIXPKGS`],
/// and each store path for the provenance file.
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub gifski: PathBuf,
    pub store: Vec<(String, String)>,
}

/// **The tools, built by `nix_out`**, ffmpeg's version checked.
pub fn tools(nix_out: impl Fn(&str) -> Result<PathBuf, String>) -> Result<Tools, String> {
    let mut store = Vec::new();
    let mut take = |attr: &str| -> Result<PathBuf, String> {
        let p = nix_out(attr)?;
        store.push((attr.to_string(), p.to_string_lossy().into_owned()));
        Ok(p)
    };
    let gifski = take("gifski")?.join("bin/gifski");
    let ffmpeg = take("ffmpeg-headless.bin")?.join("bin/ffmpeg");
    let t = Tools {
        ffmpeg,
        gifski,
        store,
    };
    let out = Command::new(&t.ffmpeg)
        .args(["-hide_banner", "-version"])
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let said = String::from_utf8_lossy(&out.stdout);
    let first = said.lines().next().unwrap_or("");
    if !out.status.success() || !first.starts_with(FFMPEG) {
        return Err(format!(
            "ffmpeg is not the pinned one: {first:?}, not {FFMPEG:?}"
        ));
    }
    Ok(t)
}

/// **The stretch of the film the GIF plays**: the film's rate and how many
/// frames it has, and the frames played, each range `first..end`, in
/// order, those that meet joined.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub rate: u32,
    pub film: u64,
    pub ranges: Vec<(u64, u64)>,
}

impl Cut {
    /// How many of the film's frames it takes.
    pub fn frames(&self) -> u64 {
        self.ranges.iter().map(|(a, b)| b - a).sum()
    }

    /// How long it plays, in seconds.
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate)
    }
}

/// **`scenes` cut by the film's marks**, read off `session.mkv.cut`'s
/// `rate R`, `frames N` and `scene N first end` lines: each scene's frames
/// in the order given, which must be the film's, those that meet joined.
/// Refused for a scene the film has no frames of, for scenes out of order,
/// and for frames past the film's.
pub fn cut(text: &str, scenes: &[u8]) -> Result<Cut, String> {
    let mut rate = None;
    let mut film = None;
    let mut found: Vec<(u8, u64, u64)> = Vec::new();
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w[..] {
            ["rate", r] => rate = r.parse::<u32>().ok().filter(|&r| r > 0),
            ["frames", n] => film = n.parse::<u64>().ok(),
            ["scene", n, a, b] => {
                let parsed = (n.parse::<u8>(), a.parse::<u64>(), b.parse::<u64>());
                let (Ok(n), Ok(a), Ok(b)) = parsed else {
                    return Err(format!(
                        "the cut's line {line:?} is not `scene N first end`"
                    ));
                };
                found.push((n, a, b));
            }
            [] => {}
            _ => return Err(format!("the cut's line {line:?} is not one it writes")),
        }
    }
    let rate = rate.ok_or("the cut names no frame rate")?;
    let film = film.ok_or("the cut names no count of the film's frames")?;
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for &n in scenes {
        let &(_, a, b) = found
            .iter()
            .find(|s| s.0 == n)
            .ok_or_else(|| format!("the film has no frames of scene {n}"))?;
        if b <= a || b > film {
            return Err(format!(
                "scene {n}'s frames {a}..{b} are none, or past the film's {film}"
            ));
        }
        match ranges.last().map(|r| r.1) {
            Some(end) if a < end => {
                return Err(format!("scene {n}'s frames {a}..{b} are out of order"));
            }
            Some(end) if a == end => {
                if let Some(last) = ranges.last_mut() {
                    last.1 = b;
                }
            }
            _ => ranges.push((a, b)),
        }
    }
    if ranges.is_empty() {
        return Err("the cut takes no scene".to_string());
    }
    Ok(Cut { rate, film, ranges })
}

/// **Every value of git's `key`** in any of git's files as the repository
/// at `root` reads them, a line each. Empty when git says none.
fn git_all(root: &Path, key: &str) -> String {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--get-all", key])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// **The names no frame of the session and no line of the GIF's
/// provenance file may show**: `machine`, the machine's, and those of the
/// git set up with the repository at `root`, by [`session_names`].
pub fn shown_names(machine: &[String], root: &Path) -> Vec<String> {
    session_names(
        machine,
        &git_all(root, "user.email"),
        &git_all(root, "user.name"),
    )
}

/// **`machine`, then each address of `emails`, then the words of
/// `users`** ([`name_words`]): `emails` and `users` are git's `user.email`
/// and `user.name` values, a line each.
pub fn session_names(machine: &[String], emails: &str, users: &str) -> Vec<String> {
    let mut all = machine.to_vec();
    all.extend(
        emails
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string),
    );
    all.extend(name_words(users));
    all
}

/// **Which of `names` `text` holds outside each of `public`, matched
/// without case**: text, names and `public` lowercased, then
/// [`names::named_outside`]. Returns the names as given.
pub fn named_in(text: &str, names: &[String], public: &[&str]) -> Vec<String> {
    let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let public: Vec<String> = public.iter().map(|p| p.to_lowercase()).collect();
    let public: Vec<&str> = public.iter().map(String::as_str).collect();
    let hit = names::named_outside(text.to_lowercase().as_bytes(), &lower, &public);
    names
        .iter()
        .zip(&lower)
        .filter(|(_, l)| hit.contains(l))
        .map(|(n, _)| n.clone())
        .collect()
}

/// **The words of `text`, a user name a line**: split at every character
/// that is not a letter or a digit, those of three characters or more,
/// each once. A shorter word is too short to tell from text.
pub fn name_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for w in text.split(|c: char| !c.is_alphanumeric()) {
        if w.chars().count() >= 3 && !words.iter().any(|k| k == w) {
            words.push(w.to_string());
        }
    }
    words
}

/// **ffmpeg's arguments**: the film's frames in the cut, by their numbers,
/// timed again from the first at the film's rate, sampled at
/// [`provenance::FPS`] a second into PNGs under `dir`.
pub fn frames_args(film: &str, c: &Cut, dir: &str) -> Vec<String> {
    let pick: Vec<String> = c
        .ranges
        .iter()
        .map(|(a, b)| format!("between(n,{a},{})", b - 1))
        .collect();
    vec![
        "-v".to_string(),
        "error".to_string(),
        "-i".to_string(),
        film.to_string(),
        "-vf".to_string(),
        format!(
            "select='{}',setpts=N/({}*TB),fps={}",
            pick.join("+"),
            c.rate,
            provenance::FPS
        ),
        format!("{dir}/%06d.png"),
    ]
}

/// **What a GIF holds, by baud's own decoder**: its frames as shown, how
/// long it plays in milliseconds and its size. An error for one that does
/// not decode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoded {
    pub frames: u32,
    pub millis: u64,
    pub width: u32,
    pub height: u32,
}

/// **`bytes` decoded**, every frame: [`Decoded`].
pub fn decoded(bytes: &[u8]) -> Result<Decoded, String> {
    use baud_domain::gif::Gif;
    let gif = Gif::parse(bytes).map_err(|e| format!("the GIF does not parse: {e:?}"))?;
    let size = gif.size();
    let mut scratch = vec![0u8; gif.scratch_len()];
    let (mut frames, mut millis) = (0u32, 0u64);
    gif.decode(&mut scratch, |f| {
        frames += 1;
        millis += u64::from(f.gap_ms);
        true
    })
    .map_err(|e| format!("the GIF does not decode: {e:?}"))?;
    Ok(Decoded {
        frames,
        millis,
        width: size.width,
        height: size.height,
    })
}

/// What the encode made.
pub struct Made {
    /// The PNGs ffmpeg sampled, which gifski encoded.
    pub pngs: usize,
    pub decoded: Decoded,
}

/// **The GIF, from the film**: ffmpeg's frames of the cut under `work`,
/// gifski's GIF of them at `gif`, decoded and held to the cut: as many
/// frames at [`provenance::FPS`] as the cut runs and the GIF as long as
/// they play (`film::covers`), and as wide as gifski was told.
pub fn encode(t: &Tools, film_at: &Path, c: &Cut, work: &Path, gif: &Path) -> Result<Made, String> {
    let frames = work.join("gif-frames");
    let _ = fs::remove_dir_all(&frames);
    fs::create_dir_all(&frames).map_err(|e| format!("{}: {e}", frames.display()))?;
    let ran = Command::new(&t.ffmpeg)
        .args(frames_args(
            &film_at.to_string_lossy(),
            c,
            &frames.to_string_lossy(),
        ))
        .status()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if !ran.success() {
        return Err(format!("ffmpeg failed on the film: {ran}"));
    }
    let mut pngs: Vec<PathBuf> = fs::read_dir(&frames)
        .map_err(|e| format!("{}: {e}", frames.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    pngs.sort();
    if pngs.is_empty() {
        return Err("ffmpeg made no frames of the cut".to_string());
    }
    let made = Command::new(&t.gifski)
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
    let bytes = fs::read(gif).map_err(|e| format!("{}: {e}", gif.display()))?;
    let d = decoded(&bytes)?;
    film::covers(pngs.len(), c.seconds(), d.millis as f64 / 1000.0)?;
    let wide = width_of(&provenance::GIFSKI)?;
    if d.width != wide {
        return Err(format!(
            "the GIF is {} pixels wide, not the {wide} gifski was told",
            d.width
        ));
    }
    Ok(Made {
        pngs: pngs.len(),
        decoded: d,
    })
}

/// The `--width` among gifski's settings.
fn width_of(settings: &[&str]) -> Result<u32, String> {
    settings
        .iter()
        .position(|a| *a == "--width")
        .and_then(|i| settings.get(i + 1))
        .and_then(|w| w.parse().ok())
        .ok_or_else(|| "gifski's settings name no --width".to_string())
}

/// **What the provenance file records**, every value from the run.
pub struct Facts<'a> {
    /// The command, as typed, the folder written `<DIR>`.
    pub command: String,
    pub from: String,
    pub tree: &'a Tree,
    pub gif_sha256: String,
    pub gif_bytes: u64,
    pub made: &'a Made,
    pub film_sha256: String,
    pub film_frames: u64,
    pub cut: &'a Cut,
    pub frames_args: Vec<String>,
    pub gifski_args: Vec<String>,
    pub tools: &'a Tools,
    /// The checks the GIF passed before the file was written, in words.
    pub checks: Vec<String>,
}

/// The test that drew the film.
pub const SESSION_TEST: &str = "film::session::the_readme_session_lights_every_light";

/// **The provenance file's tables**: what `--verify` reads -- `[gif]`'s
/// `file`, `sha256` and `bytes` and `[baud]`'s `commit` -- and what made
/// the GIF.
pub fn document(f: &Facts) -> Doc {
    let mut d = Doc::default();
    d.str("", "made", &f.command);
    d.str("", "from", &f.from);
    d.str("baud", "commit", &f.tree.commit);
    d.str(
        "baud",
        "tree",
        if f.tree.dirty.is_empty() {
            "clean"
        } else {
            "dirty, recorded with --allow-dirty"
        },
    );
    d.list("baud", "dirty", &f.tree.dirty);
    d.str("gif", "file", GIF);
    d.str("gif", "sha256", &f.gif_sha256);
    d.num("gif", "bytes", f.gif_bytes);
    d.num("gif", "frames", f.made.pngs);
    d.num(
        "gif",
        "seconds",
        format!("{:.3}", f.made.decoded.millis as f64 / 1000.0),
    );
    d.num("gif", "decoded_frames", f.made.decoded.frames);
    d.num("gif", "width", f.made.decoded.width);
    d.num("gif", "height", f.made.decoded.height);
    d.str(
        "film",
        "drawn_by",
        format!("baud-app/src/film/session.rs, {SESSION_TEST}"),
    );
    d.str(
        "film",
        "file",
        "session.mkv, beside the GIF, lossless (FFV1)",
    );
    d.str("film", "sha256", &f.film_sha256);
    d.num("film", "frames", f.film_frames);
    d.num("film", "rate", f.cut.rate);
    let scenes: Vec<String> = SCENES.iter().map(u8::to_string).collect();
    d.list("cut", "scenes", &scenes);
    let ranges: Vec<String> = f
        .cut
        .ranges
        .iter()
        .map(|(a, b)| format!("{a}..{b}"))
        .collect();
    d.list("cut", "frames", &ranges);
    d.num("cut", "seconds", format!("{:.3}", f.cut.seconds()));
    d.list("encoder", "frames", &f.frames_args);
    d.list("encoder", "gifski", &f.gifski_args);
    d.str("nixpkgs", "revision", crate::run::NIXPKGS);
    for (attr, path) in &f.tools.store {
        d.str("tools", attr, path);
    }
    d.list("checks", "passed", &f.checks);
    d
}

/// The comment the file opens with.
pub fn header() -> Vec<String> {
    vec![
        format!("How {GIF} was made: the tour played without a window by baud's own"),
        "test, its frames drawn by baud's renderer, cut by the tour's scene marks and".to_string(),
        "encoded by the commands below. Every value is the run's own.".to_string(),
        String::new(),
        format!("Check it: cargo run --example demo -- --verify <path to {GIF}>"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{self, SESSION, Step};

    /// **The cut is the tour's scenes by their marks**: scenes found by
    /// number in the order given, those that meet joined into one stretch,
    /// its frames and length at the film's rate; a scene with no frames, or
    /// scenes out of order, refused. Perturbation: join ranges that do not
    /// meet and the second cut reads one range.
    #[test]
    fn the_cut_takes_the_scenes_by_their_marks() {
        let text = "rate 100\nframes 1500\nscene 6 10 400\nscene 7 400 650\nscene 8 650 1200\n\
                    scene 9 1300 1500\n";
        let c = cut(text, &[6, 7, 8]).unwrap();
        assert_eq!(
            c,
            Cut {
                rate: 100,
                film: 1500,
                ranges: vec![(10, 1200)]
            }
        );
        assert_eq!(c.frames(), 1190);
        assert!((c.seconds() - 11.9).abs() < 1e-9);
        let apart = cut(text, &[8, 9]).unwrap();
        assert_eq!(apart.ranges, [(650, 1200), (1300, 1500)]);
        assert!(cut(text, &[5]).unwrap_err().contains("scene 5"));
        assert!(cut(text, &[8, 6]).unwrap_err().contains("out of order"));
        assert!(cut("frames 9\nscene 6 1 2\n", &[6]).is_err(), "no rate");
        assert!(cut("rate 100\nscene 6 1 2\n", &[6]).is_err(), "no count");
        assert!(
            cut("rate 100\nframes 9\nscene 6 1 20\n", &[6]).is_err(),
            "past the film"
        );
    }

    /// **A user name's words of three characters or more are names**, each
    /// once, a line a name: a word of two is left out, and so is what
    /// separates the words. The names are made up for the test.
    /// Perturbation: keep the whole line and neither word is a name of its
    /// own.
    #[test]
    fn a_user_names_words_are_names() {
        assert_eq!(
            name_words("Qzfirst Al Qzlast\nQzfirst O'Qzother-Vex\n"),
            ["Qzfirst", "Qzlast", "Qzother", "Vex"]
        );
        assert_eq!(name_words(""), Vec::<String>::new());
    }

    /// **The names the session and the provenance file are held to are the
    /// machine's, git's addresses and the words of git's user names**, in
    /// that order, an empty line no name. The names are made up for the
    /// test. Perturbation: leave the user names out and `Qzfirst` and
    /// `Qzlast` are missing.
    #[test]
    fn the_shown_names_hold_the_addresses_and_the_user_names_words() {
        let machine = ["qzuser".to_string(), "qzhost".to_string()];
        assert_eq!(
            session_names(&machine, "qz@example.org\n\n", "Qzfirst Al Qzlast\n"),
            ["qzuser", "qzhost", "qz@example.org", "Qzfirst", "Qzlast"]
        );
        assert_eq!(session_names(&machine, "", ""), machine);
    }

    /// **The provenance file's check matches without case, and takes the
    /// repository's address out**: an upper-cased name and a name given
    /// upper-cased are found and returned as given, the address on the
    /// forge in another case names nobody, and the same user elsewhere is
    /// found. The names are made up for the test. The control: a name the
    /// text does not hold is not found. Perturbation: compare with case and
    /// neither `qzfirst` nor `QZHOST` is found.
    #[test]
    fn the_provenance_check_matches_without_case() {
        let n = [
            "qzfirst".to_string(),
            "QZHOST".to_string(),
            "qzuser".to_string(),
            "elsewhere".to_string(),
        ];
        let text = "made by QZFIRST on qzhost\nnix run github:QzUser/baud\n";
        assert_eq!(named_in(text, &n, &["qzuser/baud"]), ["qzfirst", "QZHOST"]);
        assert_eq!(
            named_in("see /x/QZUSER/y\n", &n, &["qzuser/baud"]),
            ["qzuser"]
        );
    }

    /// **ffmpeg is told the cut's frames by number**, timed again at the
    /// film's rate and sampled at the GIF's. Perturbation: the range's end
    /// passed as its last frame and the select reads one frame more.
    #[test]
    fn ffmpeg_takes_the_cuts_frames_by_number() {
        let c = Cut {
            rate: 100,
            film: 40,
            ranges: vec![(10, 20), (30, 31)],
        };
        let a = frames_args("in.mkv", &c, "out");
        assert_eq!(
            a[5],
            format!(
                "select='between(n,10,19)+between(n,30,30)',setpts=N/(100*TB),fps={}",
                provenance::FPS
            )
        );
        assert_eq!(a[3], "in.mkv");
        assert_eq!(a[6], "out/%06d.png");
    }

    /// **The GIF is decoded by baud's own decoder**: a two-frame GIF of one
    /// pixel, 30 ms a frame, reads as two frames, 60 ms, one by one; bytes
    /// that are not a GIF are refused; and gifski's settings name the width
    /// the GIF is held to. Perturbation: take a frame's delay as at most
    /// 10 ms and the GIF reads 20 ms.
    #[test]
    fn the_gif_is_decoded_by_bauds_decoder() {
        let mut gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00".to_vec();
        for _ in 0..2 {
            gif.extend_from_slice(b"\x21\xf9\x04\x00\x03\x00\x00\x00");
            gif.extend_from_slice(b"\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00");
        }
        gif.push(0x3b);
        assert_eq!(
            decoded(&gif),
            Ok(Decoded {
                frames: 2,
                millis: 60,
                width: 1,
                height: 1
            })
        );
        assert!(decoded(b"not a gif").is_err());
        assert_eq!(width_of(&provenance::GIFSKI), Ok(1164));
    }

    /// **The scenes cut are the whole tour, in a row**: consecutive marks
    /// of the tour, from baud's config in helix through the README, the
    /// languages, the links, the pictures, the video, `ssh demo`'s lights,
    /// fastfetch's card, btop, the fireworks and the finale's change of the
    /// trail to the closing line. Perturbation: drop scene 17 from
    /// [`SCENES`] and the closing line is not in it.
    #[test]
    fn the_gif_plays_the_whole_tour() {
        let scenes = session::scenes(SESSION);
        let at = |n: u8| scenes.iter().position(|s| s.0 == n).expect("the scene");
        let first = at(SCENES[0]);
        for (k, &n) in SCENES.iter().enumerate() {
            assert_eq!(at(n), first + k, "scene {n} is the next of the tour");
        }
        let steps: Vec<Step> = SCENES
            .iter()
            .flat_map(|&n| SESSION[scenes[at(n)].2.clone()].to_vec())
            .collect();
        for want in [
            Step::Run(session::HX_CONFIG),
            Step::Keys(session::DOWN),
            Step::Keys(session::UP),
            Step::Run("hx README.md"),
            Step::Keys("gg"),
            Step::Run("cat hello.txt"),
            Step::Run("cat links.txt"),
            Step::Run(session::ICAT),
            Step::Run(session::MPV),
            Step::Run("ssh demo"),
            Step::Run("fastfetch"),
            Step::Run("btop"),
            Step::Run(session::FIREWORKS),
            Step::Run(session::SED_UFO),
            Step::Run(session::CLOSING),
        ] {
            assert!(steps.contains(&want), "{want:?}");
        }
        assert_eq!(scenes.len(), SCENES.len(), "every scene of the tour");
    }

    /// **The ffmpeg pinned here is the logo films'**: the same nixpkgs
    /// revision and the same first line of its version, as
    /// `tools/baud-pictures/src/logo.rs` pins them. Perturbation: another
    /// revision in `run::NIXPKGS` and this fails.
    #[test]
    fn the_ffmpeg_is_the_logo_films_pin() {
        let logo = include_str!("../../../tools/baud-pictures/src/logo.rs");
        assert!(logo.contains(&format!(
            "pub const GIFSKI_NIXPKGS: &str = \"{}\";",
            crate::run::NIXPKGS
        )));
        assert!(logo.contains(&format!("pub const FFMPEG: &str = \"{FFMPEG}\";")));
    }
}
