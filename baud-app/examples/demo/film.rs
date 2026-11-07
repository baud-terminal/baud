//! **The film's arithmetic**, as pure functions so a test can hand them
//! any display and any window: what the recorder is told, the display it
//! names, the crop to baud's window, and the checks that the film ends at
//! the stop and that the GIF is the film from its first frame to its end
//! at its own speed.

use crate::guard::Rect;
use crate::provenance::{FPS, GIFSKI};

/// The recorder's bit rate, what the earlier takes of the README session
/// asked for: a whole display of text changing at 60 frames a second.
pub const BITRATE: u32 = 60_000_000;

/// How long the recorder films after `done`: under the 1.5 s the window
/// stays, so baud is still frontmost at the stop.
pub const TAIL: &str = "1.0";

/// The fewest frames a film of the session may hold; a whole session sends
/// hundreds.
pub const FLOOR: u32 = 100;

/// **The recorder's arguments**, its files under `dir`: the display form of
/// `scripts/trail-record.swift`, which starts before baud does and reads
/// baud's pid from the file baud.pid there at the stop.
pub fn recorder_args(dir: &str) -> Vec<String> {
    vec![
        "display".to_string(),
        format!("{dir}/baud.pid"),
        format!("{dir}/raw.mp4"),
        format!("{dir}/ready"),
        format!("{dir}/done"),
        TAIL.to_string(),
        FLOOR.to_string(),
        BITRATE.to_string(),
    ]
}

/// **The writer check's films**, made by the recorder built with
/// `-D WRITER_CHECK` -- the same writer and the same end, no capture: 31
/// frames 1/60 s apart, then the stop 2.0137 s after the last, as after a
/// still tail and off the writer's 1/600 s grid as a capture's stop is,
/// and at the last frame itself, as when a frame comes at the stop. Each
/// must play from its first frame to the stop.
pub const CHECKS: [(u32, &str); 2] = [(31, "2.0137"), (31, "0")];

/// The writer check's arguments: its film, the frames, the hold and
/// [`BITRATE`].
pub fn check_args(raw: &str, frames: u32, hold: &str) -> Vec<String> {
    vec![
        raw.to_string(),
        frames.to_string(),
        hold.to_string(),
        BITRATE.to_string(),
    ]
}

/// Where a writer check's film stops: its last frame's time, `frames - 1`
/// sixtieths, and the hold after it.
pub fn stop_of(frames: u32, hold: &str) -> Option<f64> {
    let h = hold.parse::<f64>().ok()?;
    Some(f64::from(frames.checked_sub(1)?) / 60.0 + h)
}

/// The display the recorder films: its size in points, its scale and the
/// film's size in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub pt: (f64, f64),
    pub scale: f64,
    pub px: (u32, u32),
}

/// **The display, from the recorder's line**:
/// `display 1728x1117 pt at 2.0x: 3456x2234 px, 30 applications left out`.
pub fn display(line: &str) -> Option<Display> {
    let rest = line.strip_prefix("display ")?;
    let (pt, rest) = rest.split_once(" pt at ")?;
    let (scale, rest) = rest.split_once("x: ")?;
    let (px, _) = rest.split_once(" px")?;
    let pair = |s: &str| {
        let (a, b) = s.split_once('x')?;
        Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?))
    };
    let pt = pair(pt)?;
    let (pw, ph) = px.split_once('x')?;
    Some(Display {
        pt,
        scale: scale.parse().ok()?,
        px: (pw.parse().ok()?, ph.parse().ok()?),
    })
}

/// A rectangle of the film, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crop {
    pub w: u32,
    pub h: u32,
    pub x: u32,
    pub y: u32,
}

impl Crop {
    /// ffmpeg's filter for it.
    pub fn filter(&self) -> String {
        format!("crop={}:{}:{}:{}", self.w, self.h, self.x, self.y)
    }
}

/// **baud's window in the film's pixels**: its frame in points times the
/// display's scale, each edge on an even pixel, as H.264's 4:2:0 frames
/// are cut. A window not wholly on the display the recorder films is
/// refused: part of it would be off the film.
pub fn crop(window: &Rect, d: &Display) -> Result<Crop, String> {
    let on = window.x >= 0.0
        && window.y >= 0.0
        && window.x + window.w <= d.pt.0
        && window.y + window.h <= d.pt.1
        && window.w > 0.0
        && window.h > 0.0;
    if !on {
        return Err(format!(
            "baud's window ({:.0},{:.0} {:.0}x{:.0} pt) is not wholly on the main display \
             ({:.0}x{:.0} pt), which the recorder films",
            window.x, window.y, window.w, window.h, d.pt.0, d.pt.1
        ));
    }
    let even = |v: f64| ((v * d.scale).round() as u32) & !1;
    let c = Crop {
        x: even(window.x),
        y: even(window.y),
        w: even(window.w),
        h: even(window.h),
    };
    if c.x + c.w > d.px.0 || c.y + c.h > d.px.1 {
        return Err(format!(
            "the crop {} passes the film's {}x{} px",
            c.filter(),
            d.px.0,
            d.px.1
        ));
    }
    Ok(c)
}

/// **ffmpeg's arguments**: the film cropped to baud's window and sampled at
/// [`FPS`] -- each frame of the GIF the film's frame at that moment, the
/// last one held until the next -- as numbered PNGs. Nothing trimmed and
/// nothing padded.
pub fn frames_args(raw: &str, c: &Crop, dir: &str) -> Vec<String> {
    vec![
        "-v".to_string(),
        "error".to_string(),
        "-i".to_string(),
        raw.to_string(),
        "-vf".to_string(),
        format!("{},fps={FPS}", c.filter()),
        format!("{dir}/%06d.png"),
    ]
}

/// **gifski's arguments**: [`GIFSKI`] exactly, then the GIF; the frames
/// follow, in order.
pub fn gifski_args(gif: &str) -> Vec<String> {
    let mut a: Vec<String> = GIFSKI.iter().map(|s| s.to_string()).collect();
    a.extend(["-o".to_string(), gif.to_string()]);
    a
}

/// **The MP4's frame rate**: a constant 60 a second, each frame the film's
/// frame at that moment and the last one held until the next, as ffmpeg's
/// `fps` filter samples the GIF's 30. Constant rather than the capture's own
/// times because YouTube's upload guidance asks for a constant rate, and the
/// capture sends no frame while nothing changes, so a still second is one
/// frame held, which players and editors seek and cut badly; the recorder
/// films at most 60 a second (`minimumFrameInterval`), so a frame moves by
/// at most half of one sixtieth, 8.3 ms, and the timeline's times, from the
/// film's first frame, are the MP4's.
pub const MP4_FPS: u32 = 60;

/// **ffmpeg's arguments for the MP4**: the film cropped to baud's window
/// and sampled at [`MP4_FPS`], encoded by libx264 with fixed settings and
/// a fixed thread count, no audio, no metadata and every bit-exact flag, so
/// the same film makes the same bytes. Nothing trimmed and nothing padded.
pub fn mp4_args(raw: &str, c: &Crop, mp4: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-v", "error", "-y", "-i"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    a.push(raw.to_string());
    a.extend(["-vf".to_string(), format!("{},fps={MP4_FPS}", c.filter())]);
    a.extend(MP4_ENCODE.iter().map(|s| s.to_string()));
    a.push(mp4.to_string());
    a
}

/// The encoder's settings, the same in the logo's MP4
/// (`tools/baud-pictures`): H.264 High at CRF 18, `slow`, 4:2:0, BT.709,
/// four threads whatever the machine has, and nothing of the run written
/// into the file.
pub const MP4_ENCODE: [&str; 29] = [
    "-an",
    "-c:v",
    "libx264",
    "-preset",
    "slow",
    "-crf",
    "18",
    "-pix_fmt",
    "yuv420p",
    "-colorspace",
    "bt709",
    "-color_primaries",
    "bt709",
    "-color_trc",
    "bt709",
    "-threads",
    "4",
    "-fflags",
    "+bitexact",
    "-flags:v",
    "+bitexact",
    "-map_metadata",
    "-1",
    "-map_chapters",
    "-1",
    "-movflags",
    "+faststart",
    "-f",
    "mp4",
];

/// **The MP4 is the film, start to end, at its own speed**: as many frames
/// at [`MP4_FPS`] as the film runs, to within one and a half, and it plays
/// as long as those frames, to within one frame.
pub fn mp4_covers(frames: usize, film: f64, mp4: f64) -> Result<(), String> {
    let fps = f64::from(MP4_FPS);
    let at = frames as f64 / fps;
    if (at - film).abs() > 1.5 / fps {
        return Err(format!(
            "{frames} frames at {MP4_FPS} a second run {at:.3} s, but the film runs {film:.3} s"
        ));
    }
    if (mp4 - at).abs() > 1.0 / fps {
        return Err(format!(
            "the MP4 plays {mp4:.3} s, but its {frames} frames at {MP4_FPS} a second run {at:.3} s"
        ));
    }
    Ok(())
}

/// **Where the film starts on the host clock**, from the recorder's line
/// `film starts at host T ns, read at host R ns`: the first frame's time and
/// the clock read beside it, which must be within a second of it, or the
/// frame's time is not on the host clock and nothing can be laid on it.
pub fn film_start(lines: &[String]) -> Result<(u64, u64), String> {
    let l = lines
        .iter()
        .find(|l| l.starts_with("film starts at host "))
        .ok_or("the recorder never said where its film starts on the host clock")?;
    let rest = l.strip_prefix("film starts at host ").unwrap_or_default();
    let (t, rest) = rest
        .split_once(" ns, read at host ")
        .ok_or(format!("{l:?}"))?;
    let r = rest.strip_suffix(" ns").ok_or(format!("{l:?}"))?;
    let (t, r) = (
        t.parse::<u64>().map_err(|_| format!("{l:?}"))?,
        r.parse::<u64>().map_err(|_| format!("{l:?}"))?,
    );
    if t.abs_diff(r) > 1_000_000_000 {
        return Err(format!(
            "the film's first frame is {} s from the host clock read beside it, so it is not \
             on that clock",
            t.abs_diff(r) / 1_000_000_000
        ));
    }
    Ok((t, r))
}

/// **How long a film plays**, ffprobe's `-show_entries format=duration -of
/// csv=p=0`: the MP4's edit list, which the recorder ends at the stop, and
/// what ffmpeg samples. The packets do not stop there: the writer gives the
/// last one the gap before it as its duration, and after a still tail that
/// is the whole tail again, so their times and durations add up to a film
/// that runs past the stop by as long.
pub fn length(text: &str) -> Option<f64> {
    let s = text.trim().parse::<f64>().ok()?;
    (s.is_finite() && s > 0.0).then_some(s)
}

/// **Where the recorder says its film stops**, from the line it prints
/// once the film is written: `film 22.941667 s, from the first frame to
/// the stop`.
pub fn said_film(lines: &[String]) -> Option<f64> {
    lines.iter().find_map(|l| {
        l.strip_prefix("film ")?
            .strip_suffix(" s, from the first frame to the stop")?
            .parse::<f64>()
            .ok()
    })
}

/// **The film plays to the stop and no further**: its length as ffprobe
/// reads it ([`length`]) is where it stops, to within 2 ms, while a padded
/// tail or a cut one is off by the tail. The writer keeps its films' times
/// in 600ths of a second (ffprobe's `time_base` for the trail clips and
/// for the writer check's films), and a stop between two reads as the
/// nearest: 2.5137 s as 2.513333, 0.5071 s as 0.506667. 2 ms is more than
/// the start and the stop each rounded by half a 600th.
pub fn ends_at_stop(plays: f64, stop: f64) -> Result<(), String> {
    if (plays - stop).abs() > 0.002 {
        return Err(format!(
            "the film plays {plays:.3} s, but it stops {stop:.3} s after its first frame"
        ));
    }
    Ok(())
}

/// **The GIF is the film, start to end, at its own speed**: as many frames
/// at [`FPS`] as the film runs, to within one and a half frames, and the
/// GIF plays as long as those frames, to within 50 ms, GIF delays being
/// whole hundredths.
pub fn covers(frames: usize, film: f64, gif: f64) -> Result<(), String> {
    let at = frames as f64 / f64::from(FPS);
    if (at - film).abs() > 1.5 / f64::from(FPS) {
        return Err(format!(
            "{frames} frames at {FPS} a second run {at:.3} s, but the film runs {film:.3} s"
        ));
    }
    if (gif - at).abs() > 0.05 {
        return Err(format!(
            "the GIF plays {gif:.3} s, but its {frames} frames at {FPS} a second run {at:.3} s"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The display is read from the line the recorder prints**, and the
    /// recorder prints it: the format in `scripts/trail-record.swift` is
    /// the one parsed here. Perturbation: change the Swift line's ` pt at `
    /// and the second check fails.
    #[test]
    fn the_display_is_the_line_the_recorder_prints() {
        let d = display("display 1728x1117 pt at 2.0x: 3456x2234 px, 30 applications left out")
            .unwrap();
        assert_eq!(
            d,
            Display {
                pt: (1728.0, 1117.0),
                scale: 2.0,
                px: (3456, 2234)
            }
        );
        let swift = include_str!("../../../scripts/trail-record.swift");
        assert!(swift.contains(
            "print(\"display \\(d.width)x\\(d.height) pt at \\(scale)x: \\(cfg.width)x\\(cfg.height) px, \\(c.applications.count) applications left out\")"
        ));
        assert!(swift.contains("a.count == 9, a[1] == \"display\""));
        assert_eq!(recorder_args("<scratch>").len() + 1, 9, "argv[0] and eight");
        assert_eq!(display("window (0,0) at 2.0x"), None);
        assert!(swift.contains(
            "print(String(format: \"film %.6f s, from the first frame to the stop\", f))"
        ));
        assert!(swift.contains("guard a.count == 5, let n = Int(a[2])"));
        assert_eq!(check_args("<scratch>/w.mp4", 31, "0").len() + 1, 5);
    }

    /// **The film ends at the stop**: the recorder's line is read, a film
    /// that plays to it is taken, and one padded or cut by its tail is
    /// refused. The numbers are a writer's with and without the end: 31
    /// frames and a 2.01 s hold play 2.51 s ended, 4.52 s not, and its
    /// packets add up to 4.52 s either way; without the frame written
    /// again, 0.517 s. Perturbation: widen the tolerance to two seconds and
    /// the cut film passes.
    #[test]
    fn the_film_ends_at_the_stop() {
        let said = vec![
            "first frame".to_string(),
            "held the last frame 2.010 s, to the stop".to_string(),
            "film 2.510000 s, from the first frame to the stop".to_string(),
        ];
        let stop = said_film(&said).unwrap();
        assert!((stop - 2.51).abs() < 1e-9, "{stop}");
        assert_eq!(said_film(&said[..2]), None);
        assert_eq!(stop_of(31, "2.01"), Some(30.0 / 60.0 + 2.01));
        assert_eq!(stop_of(CHECKS[0].0, CHECKS[0].1), Some(0.5 + 2.0137));
        assert_eq!(stop_of(31, "0"), Some(0.5));
        assert_eq!(stop_of(0, "1"), None);
        assert_eq!(length("2.510000\n"), Some(2.51));
        assert_eq!(length("N/A"), None);
        assert_eq!(length("0.000000"), None);
        assert_eq!(ends_at_stop(2.51, stop), Ok(()));
        assert_eq!(ends_at_stop(2.5115, stop), Ok(()));
        assert_eq!(ends_at_stop(2.513333, 2.5137), Ok(()), "a 600th's rounding");
        assert!(ends_at_stop(4.52, stop).is_err(), "padded by its tail");
        assert!(ends_at_stop(0.516667, stop).is_err(), "its tail cut");
        let swift = include_str!("../../../scripts/trail-record.swift");
        assert!(swift.contains("writer?.endSession(atSourceTime: end)"));
    }

    /// **The crop is baud's window in pixels, on even edges**, and a window
    /// that hangs off the display is refused. The earlier take's window,
    /// 471,161 786x572 pt at 2x, is 1572x1144 at 942,322 -- the size of its
    /// film. Perturbation: skip the bounds check and the window at x -10
    /// gives a crop at a wrapped offset.
    #[test]
    fn the_crop_is_baud_s_window_in_pixels() {
        let d = Display {
            pt: (1728.0, 1117.0),
            scale: 2.0,
            px: (3456, 2234),
        };
        let w = Rect {
            x: 471.0,
            y: 161.0,
            w: 786.0,
            h: 572.0,
        };
        let c = crop(&w, &d).unwrap();
        assert_eq!(
            c,
            Crop {
                w: 1572,
                h: 1144,
                x: 942,
                y: 322
            }
        );
        assert_eq!(c.filter(), "crop=1572:1144:942:322");
        let odd = Rect {
            x: 10.25,
            y: 3.75,
            w: 100.5,
            h: 50.25,
        };
        let c = crop(&odd, &Display { scale: 1.0, ..d }).unwrap();
        assert!(
            [c.x, c.y, c.w, c.h].iter().all(|v| v.is_multiple_of(2)),
            "{c:?}"
        );
        for off in [
            Rect { x: -10.0, ..w },
            Rect { y: 600.0, ..w },
            Rect { x: 1000.0, ..w },
            Rect { w: 0.0, ..w },
        ] {
            assert!(crop(&off, &d).is_err(), "{off:?}");
        }
    }

    /// **The encoder is told exactly the settings, and nothing trims or
    /// pads**: gifski's ten arguments, then `-o`; ffmpeg crops and samples
    /// at 30 and does nothing else.
    #[test]
    fn the_encoder_is_told_exactly_the_settings() {
        let g = gifski_args("<scratch>/hero.gif");
        assert_eq!(
            g.join(" "),
            "--quality 80 --lossy-quality 100 --motion-quality 100 --fps 30 --width 1164 \
             -o <scratch>/hero.gif"
        );
        let c = Crop {
            w: 320,
            h: 200,
            x: 160,
            y: 100,
        };
        let f = frames_args("<scratch>/raw.mp4", &c, "<scratch>/frames");
        assert_eq!(f[5], "crop=320:200:160:100,fps=30");
        let all = f.join(" ");
        for edit in ["trim", "tpad", "setpts", "scale", "-ss", "-t ", "-to"] {
            assert!(!all.contains(edit), "{edit} in {all}");
        }
    }

    /// **The GIF is the film as it plays, start to end**: the earlier
    /// take's 688 frames at 30 cover its 22.942 s and play 22.93 s; a GIF
    /// a second short, a film twice as long, or half the frames are each
    /// refused. Perturbation: drop the check of the frames against the film
    /// and the film twice as long passes.
    #[test]
    fn the_gif_is_the_film_start_to_end() {
        let s = length("22.941667\n").unwrap();
        assert!((s - 22.941667).abs() < 1e-6, "{s}");
        assert_eq!(length(""), None);
        assert_eq!(covers(688, s, 22.93), Ok(()));
        assert!(covers(688, s, 21.93).is_err());
        assert!(covers(688, 2.0 * s, 22.93).is_err());
        assert!(covers(344, s, 11.47).is_err());
    }
}
