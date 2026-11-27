//! **The take's timeline**, `NAME.timeline.tsv` beside `--record`'s
//! `NAME.mp4`: what reached the screen and when, on the film's own clock,
//! for `tools/scripts/demo-sound.sh` to lay each sound at. A header and a row a
//! line, seven tab-separated columns:
//!
//! ```text
//! t_ms  event  detail  x_px  y_px  x2_px  y2_px
//! ```
//!
//! - `t_ms`: milliseconds from the film's first frame, whose time on the
//!   host clock the recorder prints ([`crate::film::film_start`]), to when
//!   the row's thing was shown -- not when it was sent;
//! - `event`: one of [`EVENTS`];
//! - `detail`: what it was -- the trail's kind, `CD on`, the scene's
//!   command -- and `-` for a key, whose character is never written
//!   (`take.rs`), and for nothing;
//! - the four positions: the window's pixels from its top left, the MP4's
//!   own, which is cropped to the window; a trail's start and end, a
//!   picture's or a selection's corners; empty where a row has none.
//!
//! **Where each row comes from.** baud writes what it showed into the file
//! `BAUD_TAKE` names (`crates/baud-app/src/take.rs`), each row stamped on the host
//! clock as the frame that carried it was shown: `key`, `enter`, `paste`,
//! `trail`, `fireworks`, `copy`, `picture` and `light`. The demo adds
//! `scene`, one row a typed command as its first key is posted, its time
//! taken on the demo's clock and carried onto the host's by the recorder's
//! line ([`Clock`]). A `picture` while a scene runs `mpv` is mpv's video,
//! so its `shown` and `gone` are written `video start` and `video end`.
//! `ufo` rows are not written: the saucer's flight is the trail's
//! (`Effect::Ufo` in `baud-render`), and its per-frame row would be stamped
//! where `take.rs` stamps a trail, from its position each frame.
//!
//! **How close a time is**:
//! - a frame shown just before the next is committed can stamp that next
//!   frame's rows, so a row may be up to one display period early;
//! - a picture's corners are its placements laid out left to right, as
//!   `Renderer` lays them before a row drawn right to left mirrors them;
//! - a scene's time carries the recorder's line's trip through the pipe
//!   to the demo, a few milliseconds, as [`Clock`] says.

use std::time::Instant;

/// The header, the columns in their order.
pub const HEADER: &str = "t_ms\tevent\tdetail\tx_px\ty_px\tx2_px\ty2_px";

/// Every event a row may name.
pub const EVENTS: [&str; 11] = [
    "key",
    "enter",
    "trail",
    "copy",
    "paste",
    "light",
    "picture",
    "fireworks",
    "ufo",
    "video",
    "scene",
];

/// One row on the host clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub host_ns: u64,
    pub event: String,
    pub detail: String,
    pub pos: [Option<i64>; 4],
}

/// **baud's journal read**: a row a line, seven fields, the first the host
/// clock in nanoseconds and an empty position field none.
pub fn journal(text: &str) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        let bad = || {
            format!(
                "line {} of baud's take journal is not a row: {line:?}",
                n + 1
            )
        };
        if f.len() != 7 {
            return Err(bad());
        }
        let host_ns = f[0].parse::<u64>().map_err(|_| bad())?;
        let mut pos = [None; 4];
        for (k, p) in f[3..].iter().enumerate() {
            if !p.is_empty() {
                pos[k] = Some(p.parse::<i64>().map_err(|_| bad())?);
            }
        }
        rows.push(Row {
            host_ns,
            event: f[1].to_string(),
            detail: f[2].to_string(),
            pos,
        });
    }
    Ok(rows)
}

/// **The demo's clock carried onto the host's**: the recorder's line was
/// read at `read`, and said the host clock stood at `host_ns` when it was
/// printed, so an instant is that reading moved by the time between. The
/// pipe's delay between the two is in every scene's time.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub read: Instant,
    pub host_ns: u64,
}

impl Clock {
    pub fn host(&self, at: Instant) -> u64 {
        match at.checked_duration_since(self.read) {
            Some(d) => self.host_ns.saturating_add(d.as_nanos() as u64),
            None => self
                .host_ns
                .saturating_sub(self.read.duration_since(at).as_nanos() as u64),
        }
    }
}

/// A scene's row: the command typed, at the host time its first key was
/// posted.
pub fn scene(host_ns: u64, command: &str) -> Row {
    Row {
        host_ns,
        event: "scene".to_string(),
        detail: clean(command),
        pos: [None; 4],
    }
}

/// A detail with no tab, newline or control in it, and `-` for none.
fn clean(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    match s.trim() {
        "" => "-".to_string(),
        t => t.to_string(),
    }
}

/// `picture` rows inside a scene that runs `mpv`, as `video start` and
/// `video end`; the scenes are among `rows`, which are in time order.
fn videos(rows: &mut [Row]) {
    let mut mpv = false;
    for r in rows.iter_mut() {
        if r.event == "scene" {
            mpv = r.detail == "mpv" || r.detail.starts_with("mpv ");
        } else if mpv && r.event == "picture" {
            r.event = "video".to_string();
            r.detail = match r.detail.as_str() {
                "shown" => "start".to_string(),
                _ => "end".to_string(),
            };
        }
    }
}

/// **The timeline's text**: [`HEADER`], then every row in time order, the
/// journal's order kept between rows of one time, each `t_ms` the row's
/// host time less the film's first frame's, `first_ns`, in whole
/// milliseconds. A row before the first frame is not on the film and is
/// refused, as is an event not in [`EVENTS`].
pub fn text(first_ns: u64, mut rows: Vec<Row>) -> Result<String, String> {
    rows.sort_by_key(|r| r.host_ns);
    videos(&mut rows);
    let mut out = String::from(HEADER);
    out.push('\n');
    for r in &rows {
        if !EVENTS.contains(&r.event.as_str()) {
            return Err(format!("{:?} is not an event of the timeline", r.event));
        }
        let since = r.host_ns.checked_sub(first_ns).ok_or_else(|| {
            format!(
                "a {} row is {} ms before the film's first frame, so not on the film",
                r.event,
                (first_ns - r.host_ns) / 1_000_000
            )
        })?;
        let ms = (since + 500_000) / 1_000_000;
        let p = |v: Option<i64>| v.map_or_else(String::new, |v| v.to_string());
        out.push_str(&format!(
            "{ms}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            r.event,
            clean(&r.detail),
            p(r.pos[0]),
            p(r.pos[1]),
            p(r.pos[2]),
            p(r.pos[3])
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The timeline is the header and the rows in the spec's column
    /// order, timed from the film's first frame**: baud's journal, read as
    /// the app writes it, and a scene from the demo's clock, put in time
    /// order with each time the host clock less the first frame's, an mpv
    /// scene's picture written as video. The film starts 5 s after the
    /// launch here, so a timeline counted from the earliest row -- the
    /// launch's light -- is off by every row. Perturbation: count `t_ms`
    /// from `rows[0]` in place of `first_ns`, and the times are wrong.
    #[test]
    fn the_timeline_is_the_rows_in_column_order_on_the_film_clock() {
        let first = 9_000_000_000_u64;
        let journal_text = "\
            10234000000\tkey\th\t412\t980\t\t\n\
            10270400000\tkey\tspace\t436\t980\t\t\n\
            9600000000\tlight\tOH on\t\t\t\t\n\
            11100000000\ttrail\tplasma\t508\t980\t508\t40\n\
            18000000000\tpicture\tshown\t120\t200\t900\t700\n\
            18100000000\tpicture\tgone\t\t\t\t\n";
        let mut rows = journal(journal_text).unwrap();
        let read = Instant::now();
        let clock = Clock {
            read,
            host_ns: 17_000_000_000,
        };
        rows.push(scene(
            clock.host(read + std::time::Duration::from_millis(500)),
            "mpv logo.mp4",
        ));
        assert_eq!(
            text(first, rows).unwrap(),
            "t_ms\tevent\tdetail\tx_px\ty_px\tx2_px\ty2_px\n\
             600\tlight\tOH on\t\t\t\t\n\
             1234\tkey\th\t412\t980\t\t\n\
             1270\tkey\tspace\t436\t980\t\t\n\
             2100\ttrail\tplasma\t508\t980\t508\t40\n\
             8500\tscene\tmpv logo.mp4\t\t\t\t\n\
             9000\tvideo\tstart\t120\t200\t900\t700\n\
             9100\tvideo\tend\t\t\t\t\n"
        );
        let early = journal("8999000000\tkey\tq\t1\t2\t\t\n").unwrap();
        assert!(text(first, early).is_err(), "before the first frame");
        let ufo_less = journal("9999000000\tsaucer\t-\t\t\t\t\n").unwrap();
        assert!(text(first, ufo_less).is_err(), "an event not in the list");
        assert!(journal("1\tkey\th\t1\n").is_err(), "a short row");
        assert_eq!(HEADER.split('\t').count(), 7);
    }
}
