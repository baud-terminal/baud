//! Record what `kitten icat` writes to a terminal, through a real PTY: the
//! `record-kitten-icat` example.
//!
//! The kitty graphics stream is `APC G <keys> ; <base64> ST`. `kitten icat`
//! chooses what to send from the window's size in pixels, which it reads from
//! the PTY's `winsize` (`ws_xpixel`, `ws_ypixel`), so the PTY here carries the
//! pixels baud's app reports: `cols * 17` by `rows * 34`, the default 13 pt
//! cell at 2x. `--transfer-mode=stream` is the medium that needs no shared
//! file (`t=d`, direct), and with it and no `--detect-support` kitten sends no
//! query, so nothing has to answer it.
//!
//! kitten's default, `--transfer-mode=detect`, queries first: three `a=q`, one
//! per medium, then the Primary DA, and it waits up to ten seconds for the DA.
//! The `detect-*` cases answer as baud would: baud's DA is `CSI ? 1 ; 2 c`,
//! or `CSI ? 6 ; 4 c` with `[images] advertise_sixel`, and baud answers no
//! `a=q` today. `detect-direct` also answers `OK` to the direct query alone, which
//! is what a baud implementing `t=d` and no other medium would send.
//!
//! kitty and the input pictures are pinned by nixpkgs revision, and every
//! input is checked against its sha256 before it is used; a case whose input
//! is a macOS system file is skipped where that file differs or is missing.
//!
//! `--scan ROOT...` records nothing and counts instead: every PNG under the
//! roots, by its `IHDR`, that kitten would send as-is at each window, and how
//! many of those are interlaced (Adam7), by count and by bytes. kitten's
//! `--fit` defaults to `width`, so that is every PNG no wider than the window,
//! whatever its height; it scales a wider one and sends it raw, which cannot
//! carry Adam7. A multi-frame PNG is counted as sent as-is, which it is not.
//!
//! ```text
//! record-kitten-icat --out DIR
//! record-kitten-icat --scan /System/Library /Library /Applications
//! ```

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

use super::args::{Conv, Need, Opt, Parser, Rest, Takes};
use super::pty::{Child, Environ, Lookup, Spawn, clock};
use super::py;
use crate::key_latency::sha256;

const NIXPKGS: &str = "github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a";
/// baud-app's default cell, device pixels: 13 pt at 2x.
const CELL: (i64, i64) = (17, 34);
const WINDOWS: [(&str, i64, i64); 4] = [
    // The smallest grid at that cell whose text area holds 1920x1080.
    ("113x32", 113, 32),
    ("80x24", 80, 24),
    // 1920 wide and 816 high: kitten fits to width alone, so a 1920x1080
    // picture still goes whole.
    ("113x24", 113, 24),
    // The smallest that holds 3840x2160: a full-screen window on a 4K panel.
    ("226x64", 226, 64),
];

/// Where an input comes from.
enum Input {
    /// An attribute of the pinned nixpkgs, and the file under its output.
    Nix(&'static str, &'static str),
    /// A file on this machine.
    File(&'static str),
}

const INPUTS: [(&str, Input, &str); 3] = [
    // nixos-artwork, CC-BY-SA-4.0: 1920x1080 RGB, full-window at 113x32.
    (
        "mosaic-blue",
        Input::Nix(
            "nixos-artwork.wallpapers.mosaic-blue",
            "share/wallpapers/mosaic-blue/contents/images/1920x1080.png",
        ),
        "c596cd2bcb37fe8a11bf27871b185c6279e27de1808809d48f0f448c94d66cb1",
    ),
    // nixos-artwork, CC-BY-SA-4.0: 3840x2160 RGBA, wider than all but 226x64.
    (
        "waterfall",
        Input::Nix(
            "nixos-artwork.wallpapers.waterfall",
            "share/wallpapers/waterfall/contents/images/3840x2160.png",
        ),
        "50b14d5193d4f648431baaed90cb242dee6c62951cac956f705bc4929bd78cc3",
    ),
    // macOS 27.0: 1160x580 RGBA, interlace method 1 (Adam7); narrower than
    // every window.
    (
        "skyglow-adam7",
        Input::File(
            "/System/Library/PrivateFrameworks/GeoKit.framework/Versions/A/Resources/UTC-08.00-Skyglow@2x.png",
        ),
        "e035d914fa659efae67108035fa97d31b756fc71cce807e7844d475702e74d7f",
    ),
];

/// Whether a `detect` case answers kitten's direct query.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direct {
    /// It answers `OK`.
    Answered,
    /// It stays silent, as baud does today.
    Silent,
}

/// A recording mode: its name, kitten's flags, the Primary DA's reply and
/// whether the direct query is answered.
pub type Mode = (
    &'static str,
    &'static [&'static str],
    Option<&'static [u8]>,
    Direct,
);

const MODES: [Mode; 5] = [
    ("stream", &["--transfer-mode=stream"], None, Direct::Silent),
    ("detect-da12", &[], Some(b"\x1b[?1;2c"), Direct::Silent),
    ("detect-da64", &[], Some(b"\x1b[?6;4c"), Direct::Silent),
    ("detect-direct", &[], Some(b"\x1b[?6;4c"), Direct::Answered),
    // Unicode placeholders, as through tmux: the image with `a=T,U=1` and
    // then U+10EEEE cells, each with its row, column and id-byte diacritics.
    (
        "placeholder",
        &["--transfer-mode=stream", "--unicode-placeholder"],
        None,
        Direct::Silent,
    ),
];

const PARSER: Parser = Parser {
    prog: "record-kitten-icat",
    usage: "usage: record-kitten-icat [-h] [--out OUT] [--scan SCAN [SCAN ...]]
                          [--timeout TIMEOUT]
",
    help: "
options:
  -h, --help            show this help message and exit
  --out OUT
  --scan SCAN [SCAN ...]
  --timeout TIMEOUT
",
    opts: &[
        Opt {
            flag: "--out",
            takes: Takes::One(Conv::Text),
            need: Need::Optional,
        },
        Opt {
            flag: "--scan",
            takes: Takes::Many,
            need: Need::Optional,
        },
        Opt {
            flag: "--timeout",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
    ],
    rest: Rest::Nothing,
};

/// The first index at or past `from` of a byte of `stop`.
fn run_end(b: &[u8], from: usize, stop: &[u8]) -> usize {
    b[from..]
        .iter()
        .position(|c| stop.contains(c))
        .map_or(b.len(), |n| from + n)
}

/// `re.findall(rb"\x1b_G([^;\x1b]*a=q[^;\x1b]*);", out)`: the keys of each
/// `a=q`.
pub fn queries(out: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(i) = py::find(&out[at..], b"\x1b_G") {
        let keys = at + i + 3;
        let end = run_end(out, keys, b";\x1b");
        if out.get(end) == Some(&b';') && py::find(&out[keys..end], b"a=q").is_some() {
            found.push(&out[keys..end]);
            at = end + 1;
        } else {
            at = keys;
        }
    }
    found
}

/// The answers to kitten's detection burst, once its DA has been sent. `Err`
/// is the exception the script died of on keys it could not read.
pub fn reply(out: &[u8], da: &[u8], direct: Direct) -> Result<Vec<u8>, String> {
    let mut ans = Vec::new();
    for keys in queries(out) {
        let mut medium: Option<&[u8]> = None;
        let mut id: Option<&[u8]> = None;
        for pair in keys.split(|b| *b == b',') {
            let Some(eq) = pair.iter().position(|b| *b == b'=') else {
                return Err(
                    "dictionary update sequence element has length 1; 2 is required".to_string(),
                );
            };
            let (k, v) = (&pair[..eq], &pair[eq + 1..]);
            if k == b"t" {
                medium = Some(v);
            }
            if k == b"i" {
                id = Some(v);
            }
        }
        if direct == Direct::Answered && medium.unwrap_or(b"d") == b"d" {
            let id = id.ok_or_else(|| "KeyError: b'i'".to_string())?;
            ans.extend_from_slice(b"\x1b_Gi=");
            ans.extend_from_slice(id);
            ans.extend_from_slice(b";OK\x1b\\");
        }
    }
    ans.extend_from_slice(da);
    Ok(ans)
}

/// `re.findall(rb"\x1b_G([^;\x1b]*)(?:;([^\x1b]*))?\x1b\\", raw)`: each
/// command's keys and payload.
pub fn commands(raw: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(i) = py::find(&raw[at..], b"\x1b_G") {
        let keys = at + i + 3;
        let keys_end = run_end(raw, keys, b";\x1b");
        let (payload, end) = if raw.get(keys_end) == Some(&b';') {
            let e = run_end(raw, keys_end + 1, b"\x1b");
            (&raw[keys_end + 1..e], e)
        } else {
            (&raw[keys_end..keys_end], keys_end)
        };
        if raw[end..].starts_with(b"\x1b\\") {
            found.push((&raw[keys..keys_end], payload));
            at = end + 2;
        } else {
            at = keys;
        }
    }
    found
}

/// The line that says what a recording holds.
pub fn summary(raw: &[u8]) -> Result<String, String> {
    let cmds = commands(raw);
    let Some((first, _)) = cmds.first() else {
        return Ok("no APC G command".to_string());
    };
    let first = std::str::from_utf8(first).map_err(|e| format!("'utf-8' codec: {e}"))?;
    let payload: usize = cmds.iter().map(|(_, p)| p.len()).sum();
    let largest = cmds.iter().map(|(_, p)| p.len()).max().unwrap_or(0);
    Ok(format!(
        "{} commands, first keys {first}, base64 {payload} bytes, largest chunk {largest}",
        cmds.len()
    ))
}

/// One PNG a scan found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Png {
    /// Its `IHDR` width, and its interlace method.
    pub head: (u32, u8),
    /// Its size on disk.
    pub bytes: u64,
}

/// What a file's first 29 bytes say: nothing for a file that is no PNG, and
/// the script's `struct.error` for a PNG cut short inside its `IHDR`.
pub fn ihdr(head: &[u8]) -> Result<Option<(u32, u8)>, String> {
    if head.get(..8) != Some(b"\x89PNG\r\n\x1a\n".as_slice()) || head.get(12..16) != Some(b"IHDR") {
        return Ok(None);
    }
    if head.len() < 29 {
        return Err("unpack requires a buffer of 13 bytes".to_string());
    }
    let w = u32::from_be_bytes([head[16], head[17], head[18], head[19]]);
    Ok(Some((w, head[28])))
}

/// Every PNG under `dir`, each file once: `os.walk`, so a link to a directory
/// is not followed, and `os.stat`, so a link to a file is.
fn walk(dir: &Path, seen: &mut BTreeSet<(u64, u64)>, pngs: &mut Vec<Png>) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_link = entry.file_type().is_ok_and(|t| t.is_symlink());
        let meta = std::fs::metadata(&path);
        if meta.as_ref().is_ok_and(std::fs::Metadata::is_dir) {
            if !is_link {
                walk(&path, seen, pngs)?;
            }
            continue;
        }
        let name = entry.file_name();
        if !name.to_string_lossy().to_lowercase().ends_with(".png") {
            continue;
        }
        let Ok(meta) = meta else {
            continue;
        };
        if !seen.insert((meta.dev(), meta.ino())) {
            continue;
        }
        let mut head = Vec::with_capacity(29);
        let read = std::fs::File::open(&path).and_then(|f| f.take(29).read_to_end(&mut head));
        if read.is_err() {
            continue;
        }
        if let Some(found) = ihdr(&head)? {
            pngs.push(Png {
                head: found,
                bytes: meta.size(),
            });
        }
    }
    Ok(())
}

/// The lines a scan prints, or the exception it died of on the line before:
/// a window no PNG fits divides by zero.
pub fn scan_lines(pngs: &[Png]) -> (Vec<String>, Option<String>) {
    let interlaced = pngs.iter().filter(|p| p.head.1 == 1).count();
    let mut lines = vec![format!("{} PNGs, {interlaced} interlaced", pngs.len())];
    for (window, cols, _) in WINDOWS {
        let xp = u64::try_from(cols * CELL.0).expect("a window's width");
        let fit: Vec<&Png> = pngs.iter().filter(|p| u64::from(p.head.0) <= xp).collect();
        let (n, b) = (fit.len(), fit.iter().map(|p| p.bytes).sum::<u64>());
        let adam7: Vec<&&Png> = fit.iter().filter(|p| p.head.1 == 1).collect();
        let (n7, b7) = (adam7.len(), adam7.iter().map(|p| p.bytes).sum::<u64>());
        if n == 0 {
            return (lines, Some("division by zero".to_string()));
        }
        if b == 0 {
            return (lines, Some("division by zero".to_string()));
        }
        lines.push(format!(
            "{window} ({xp} wide): {n} sent as-is, {b} bytes; Adam7 {n7} ({:.2}%), {b7} bytes ({:.2}%); {} sent raw",
            100.0 * n7 as f64 / n as f64,
            100.0 * b7 as f64 / b as f64,
            pngs.len() - n
        ));
    }
    (lines, None)
}

/// `--scan`.
fn scan(roots: &[&OsString]) -> i32 {
    let mut seen = BTreeSet::new();
    let mut pngs = Vec::new();
    for root in roots {
        if let Err(e) = walk(Path::new(root), &mut seen, &mut pngs) {
            return super::died(&e);
        }
    }
    let (lines, died) = scan_lines(&pngs);
    for l in lines {
        println!("{l}");
    }
    match died {
        Some(e) => super::died(&e),
        None => 0,
    }
}

/// `nix build`'s output path for an attribute of the pinned nixpkgs.
fn nix_out(attr: &str) -> Result<String, String> {
    let out = Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths"])
        .arg(format!("{NIXPKGS}#{attr}"))
        .output()
        .map_err(|e| format!("nix: {e}"))?;
    if !out.status.success() {
        return Err(format!("nix build {attr}: {}", out.status));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(py::strip(&text).to_string())
}

/// What one run of kitten did.
pub struct Recorded {
    /// What it wrote.
    pub raw: Vec<u8>,
    /// How it ended: an exit code, or minus a signal.
    pub code: i32,
    /// How long it took.
    pub secs: f64,
}

/// The mode of that name.
pub fn mode_named(name: &str) -> Option<&'static Mode> {
    MODES.iter().find(|m| m.0 == name)
}

/// One run of `kitten icat`.
pub fn record(
    kitten: &str,
    mode: &Mode,
    image: &str,
    cols: i64,
    rows: i64,
    timeout: f64,
) -> Result<Recorded, String> {
    let (_, flags, da, direct) = *mode;
    let mut argv: Vec<OsString> = vec![kitten.into(), "icat".into()];
    argv.extend(flags.iter().map(OsString::from));
    argv.push(image.into());
    let mut env = Environ::current();
    env.set("TERM", "xterm-256color");
    let spawn = Spawn::new(Lookup::Direct, &argv, None, &env, 1)?;
    let child = Child::fork(&spawn)?;
    child.set_size(cols, rows, cols * CELL.0, rows * CELL.1)?;
    let mut raw = Vec::new();
    let mut answered = da.is_none();
    let start = clock();
    let end = start + timeout;
    let mut finished = false;
    while clock() < end {
        if !child.readable(0.2)? {
            continue;
        }
        let chunk = child.read(65536).unwrap_or_default();
        if chunk.is_empty() {
            finished = true;
            break;
        }
        raw.extend_from_slice(&chunk);
        if !answered && py::find(&raw, b"\x1b[c").is_some() {
            child.write(&reply(&raw, da.unwrap_or_default(), direct)?)?;
            answered = true;
        }
    }
    if !finished {
        let _ = child.kill(libc::SIGKILL);
        return Err(format!(
            "kitten icat {image} did not finish in {}s",
            py::repr_float(timeout)
        ));
    }
    let code = child.wait()?;
    child.close();
    Ok(Recorded {
        raw,
        code,
        secs: clock() - start,
    })
}

/// The cases to run over the inputs found: every window in stream mode, and
/// the other modes on `mosaic-blue` alone.
pub fn cases(found: &[&'static str]) -> Vec<(&'static str, &'static str, usize)> {
    let mut all = Vec::new();
    for name in found {
        for (window, _, _) in WINDOWS {
            all.push((*name, window, 0));
        }
    }
    if found.contains(&"mosaic-blue") {
        for m in 1..MODES.len() {
            all.push(("mosaic-blue", "113x32", m));
        }
    }
    all
}

/// The example.
pub fn run(args: &[OsString]) -> i32 {
    let parsed = match PARSER.parse(args) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let roots = parsed.all("--scan");
    if !roots.is_empty() {
        return scan(&roots);
    }
    let Some(out) = parsed.one("--out").filter(|o| !o.is_empty()) else {
        eprint!("{}", PARSER.usage);
        eprintln!("{}: error: --out or --scan", PARSER.prog);
        return 2;
    };
    match record_all(out, parsed.float("--timeout", 60.0)) {
        Ok(code) => code,
        Err(e) => super::died(&e),
    }
}

/// Every case, into `out`.
fn record_all(out: &OsStr, timeout: f64) -> Result<i32, String> {
    super::pty::coerce_locale();
    std::fs::create_dir_all(out).map_err(|e| format!("makedirs: {e}"))?;
    let kitty = nix_out("kitty")?;
    let kitten = format!("{kitty}/bin/kitten");
    let version = Command::new(&kitten)
        .arg("--version")
        .output()
        .map_err(|e| format!("{kitten}: {e}"))?;
    if !version.status.success() {
        return Err(format!("{kitten} --version: {}", version.status));
    }
    println!("{}", py::strip(&String::from_utf8_lossy(&version.stdout)));

    let mut paths: Vec<(&'static str, String)> = Vec::new();
    for (name, from, want) in &INPUTS {
        let path = match from {
            Input::Nix(attr, rel) => format!("{}/{rel}", nix_out(attr)?),
            Input::File(p) => (*p).to_string(),
        };
        if !Path::new(&path).exists() {
            println!("{name}: skipped, {path} is missing");
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("open: {e}"))?;
        let got = sha256::digest(&bytes);
        println!("{name}: {path}\n  input sha256 {got}");
        if got != *want {
            if matches!(from, Input::Nix(..)) {
                println!("  expected {want}");
                return Ok(1);
            }
            println!("  skipped: not the file the recordings were made from");
            continue;
        }
        paths.push((name, path));
    }
    let names: Vec<&'static str> = paths.iter().map(|(n, _)| *n).collect();
    for (name, window, m) in cases(&names) {
        let (_, cols, rows) = *WINDOWS
            .iter()
            .find(|(w, _, _)| *w == window)
            .expect("a window of the table");
        let image = &paths
            .iter()
            .find(|(n, _)| *n == name)
            .expect("an input found")
            .1;
        let mode = &MODES[m];
        let Recorded { raw, code, secs } = record(&kitten, mode, image, cols, rows, timeout)?;
        let mut dest = out.to_os_string();
        dest.push(format!("/{name}-{window}-{}.raw", mode.0));
        std::fs::write(&dest, &raw).map_err(|e| format!("open: {e}"))?;
        println!(
            "{name} {window} {}: exit {code} after {secs:.1} s, {} bytes",
            mode.0,
            raw.len()
        );
        println!("  sha256 {}", sha256::digest(&raw));
        println!("  {}", summary(&raw)?);
        if code != 0 {
            let tail = raw.len().saturating_sub(160);
            println!("  ends {}", py::bytes_repr(&raw[tail..]));
        }
    }
    Ok(0)
}
