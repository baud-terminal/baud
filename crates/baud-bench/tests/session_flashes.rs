//! **The README session's flashes, counted on the frames baud drew** (WCAG
//! 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]: the
//! film `cargo run --example demo -- --headless DIR` makes without a
//! window, every frame baud's CPU renderer drew at the period it was drawn
//! at, lossless, and the shots its `.shots` list names -- the trails' (the
//! plasma trail through the README in the container's helix), the card's
//! (fastfetch's mark drawn as a picture), the fireworks' and the clear's
//! (the saucer's flight to the top) -- each counted on its own, at 2
//! pixels a point as the window is filmed, with the title strip the session
//! composes over every frame (`crates/baud-app/src/film/chrome.rs`).
//!
//! **Each shot passes**: the verdict's reading and the chained one under
//! the 25% of the field, and the frames opaque. How many times a pixel
//! flashed in its worst second is printed, not held: a trail or a star is
//! a small area, which the area limit is for. The control is the census's
//! own: `fireworks_flashes.rs` fails iTerm2's show on the same counter.

use baud_bench::flash::{Census, Frame, Outcome, Rate, Report, Scale};
use baud_font::Px;
use std::io::Read as _;

/// A shot: its name and its first and last frames.
struct Shot {
    name: String,
    first: Frame,
    last: Frame,
}

/// The film's size, its rate and its shots, from its `.shots` list.
fn shots(text: &str) -> ((u32, u32), u32, Vec<Shot>) {
    let mut size = None;
    let mut rate = None;
    let mut shots = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let n = |i: usize| words[i].parse::<u32>().expect("a number");
        match words.as_slice() {
            ["size", _, _] => size = Some((n(1), n(2))),
            ["rate", _] => rate = Some(n(1)),
            [name, _, _] => shots.push(Shot {
                name: name.to_string(),
                first: Frame(n(1)),
                last: Frame(n(2)),
            }),
            _ => panic!("a line of the shots' list: {line}"),
        }
    }
    (size.expect("size"), rate.expect("rate"), shots)
}

#[test]
#[ignore = "a tool: `cargo run --example demo -- --headless DIR` runs it"]
fn the_readme_session_is_below_the_flash_thresholds() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
    let film = var("BAUD_SESSION_FILM");
    let ffmpeg = var("BAUD_FILM_FFMPEG");
    let list = std::fs::read_to_string(format!("{film}.shots")).expect("the shots' list");
    let ((w, h), rate, shots) = shots(&list);
    assert!(!shots.is_empty(), "no shot to count");
    let mut decode = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-i", &film])
        .args(["-f", "rawvideo", "-pix_fmt", "bgra", "-"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("ffmpeg");
    let mut out = decode.stdout.take().expect("ffmpeg's output");
    let slots = usize::try_from(w * h).expect("a frame's pixels");
    let mut bytes = vec![0u8; slots * 4];
    let mut pixels = vec![0u32; slots];
    let mut reports: Vec<(String, Report)> = Vec::new();
    let mut census: Option<(usize, Census)> = None;
    let mut frame = Frame(0);
    while out.read_exact(&mut bytes).is_ok() {
        for (p, b) in pixels.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *p = u32::from_le_bytes(*b);
        }
        if census.is_none() {
            if let Some(i) = shots.iter().position(|s| s.first == frame) {
                census = Some((i, Census::new(Px::of(w), Px::of(h), Scale(2), Rate(rate))));
            }
        }
        if let Some((i, c)) = &mut census {
            c.frame(&pixels);
            if shots[*i].last == frame {
                let (i, c) = census.take().expect("a census");
                reports.push((shots[i].name.clone(), c.finish()));
            }
        }
        frame = Frame(frame.0 + 1);
    }
    assert!(decode.wait().expect("ffmpeg").success(), "ffmpeg failed");
    assert_eq!(reports.len(), shots.len(), "every shot counted");
    for (name, r) in &reports {
        println!(
            "{name}: {} frames at {rate} a second; at a pixel in a second, general {} red {} \
             flashes; area {:.0}, strict {:.0}, of {:.0}",
            r.frames.0, r.general.flashes.0, r.red.flashes.0, r.area.0, r.strict.0, r.threshold.0
        );
        assert_eq!(r.translucent.value(), 0, "{name}: an opaque page");
        assert_eq!(r.outcome(), Outcome::Passes, "{name}: {r:?}");
        assert_eq!(r.strict_outcome(), Outcome::Passes, "{name}: {r:?}");
    }
}
