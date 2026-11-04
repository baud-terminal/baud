//! **The guard that keeps a key or a wheel turn out of every other
//! application**, as pure functions of what the window server and AppKit
//! answer, so a test can hand them any desktop.
//!
//! The keys' rule is the one the README's first recording was typed
//! under: before every key-down and every key-up, the
//! frontmost ordinary window's owner and the frontmost application must
//! both be baud's pid. The wheel's adds `scripts/readme-pointer.swift`'s:
//! the point must lie in a window of baud's with no other process's
//! ordinary window in front of it.

/// A window as `CGWindowListCopyWindowInfo` describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub owner: i32,
    pub owner_name: String,
    /// `kCGWindowLayer`: 0 is an ordinary window.
    pub layer: i64,
    pub on_screen: OnScreen,
    /// Screen points, the origin at the top left.
    pub bounds: Rect,
}

/// Whether a window is on screen (`kCGWindowIsOnscreen`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnScreen {
    Yes,
    No,
}

/// A rectangle in screen points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn holds(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.0 < self.x + self.w && p.1 >= self.y && p.1 < self.y + self.h
    }
}

/// Why the guard refused, in words for the person at the desk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String);

/// **The keys' guard**: `front` is the on-screen window list front to back
/// (`kCGWindowListOptionOnScreenOnly`, "ordered from front to back",
/// `[[source: macos-sdk-cgwindow-h]]`), `app` the frontmost application's
/// pid. Both the first ordinary window's owner and the application must be
/// `pid`.
pub fn keys(pid: i32, front: &[Window], app: Option<i32>) -> Result<(), Refused> {
    let window = front.iter().find(|w| w.layer == 0);
    let owner = window.map(|w| w.owner);
    if owner == Some(pid) && app == Some(pid) {
        return Ok(());
    }
    Err(Refused(format!(
        "the front window is {} ({}) and the front application {}, not baud's pid {pid}",
        window.map_or("nobody's".to_string(), |w| format!("pid {}'s", w.owner)),
        window.map_or("no name", |w| w.owner_name.as_str()),
        app.map_or("none".to_string(), |a| format!("pid {a}")),
    )))
}

/// **The wheel's guard**: the keys' guard, and `p` lies in a window of
/// `pid` reached walking the ordinary windows front to back while `pid`
/// owns them -- baud's own tooltip can be the frontmost and is passed over,
/// as `scripts/readme-pointer.swift` passes it over.
pub fn wheel(pid: i32, front: &[Window], app: Option<i32>, p: (f64, f64)) -> Result<(), Refused> {
    keys(pid, front, app)?;
    for w in front.iter().filter(|w| w.layer == 0) {
        if w.owner != pid {
            break;
        }
        if w.bounds.holds(p) {
            return Ok(());
        }
    }
    Err(Refused(format!(
        "the point {:.0},{:.0} is in no window of baud's pid {pid} in front of every other",
        p.0, p.1
    )))
}

/// **`pid`'s largest window on screen**: its terminal window, rather than
/// a tooltip or a panel.
pub fn frame(pid: i32, all: &[Window]) -> Option<Rect> {
    all.iter()
        .filter(|w| w.owner == pid && w.on_screen == OnScreen::Yes)
        .max_by(|a, b| (a.bounds.w * a.bounds.h).total_cmp(&(b.bounds.w * b.bounds.h)))
        .map(|w| w.bounds)
}

/// **Whether baud's window has held still**, from the frames read since
/// the session began, each with its time in seconds: `Waiting` until it
/// has been seen and has kept one frame for `hold` seconds, then
/// `Still` at the frame it was first seen at, or `Moved` from it. The
/// film begins before baud's window appears, so the launch is on film
/// where the window was first seen; a window that settles anywhere else
/// leaves no one crop that holds both. A reading of no window after the
/// first starts the hold again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Settle {
    Waiting,
    Still(Rect),
    Moved { first: Rect, now: Rect },
}

pub fn settle(readings: &[(f64, Option<Rect>)], hold: f64) -> Settle {
    let Some(first) = readings.iter().find_map(|r| r.1) else {
        return Settle::Waiting;
    };
    let Some(&(last, Some(now))) = readings.last() else {
        return Settle::Waiting;
    };
    let since = readings
        .iter()
        .rev()
        .take_while(|r| r.1 == Some(now))
        .last()
        .map_or(last, |r| r.0);
    if last - since < hold {
        Settle::Waiting
    } else if now == first {
        Settle::Still(now)
    } else {
        Settle::Moved { first, now }
    }
}

/// **The centre of [`frame`]**, where the wheel turns.
pub fn centre(pid: i32, all: &[Window]) -> Option<(f64, f64)> {
    frame(pid, all).map(|b| (b.x + b.w / 2.0, b.y + b.h / 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAUD: i32 = 500;
    const OTHER: i32 = 77;

    fn win(owner: i32, layer: i64, x: f64, y: f64, w: f64, h: f64) -> Window {
        Window {
            owner,
            owner_name: if owner == BAUD { "baud" } else { "Other" }.to_string(),
            layer,
            on_screen: OnScreen::Yes,
            bounds: Rect { x, y, w, h },
        }
    }

    /// **A key goes only when baud owns the front window and is the front
    /// application**: a menu bar or dock window above it (layer 25, 20)
    /// does not count, another application's window in front refuses, and
    /// so does baud's window in front while another application is
    /// frontmost -- the shape a stale front-application answer would let
    /// through, which is why `front_app` turns the run loop first.
    /// Perturbation: check the window alone and the third case passes.
    #[test]
    fn a_key_goes_only_to_baud_in_front() {
        let baud_front = [
            win(OTHER, 25, 0.0, 0.0, 1000.0, 24.0),
            win(BAUD, 0, 100.0, 100.0, 800.0, 600.0),
            win(OTHER, 0, 0.0, 0.0, 1000.0, 800.0),
        ];
        assert_eq!(keys(BAUD, &baud_front, Some(BAUD)), Ok(()));
        let other_front = [
            win(OTHER, 0, 0.0, 0.0, 1000.0, 800.0),
            win(BAUD, 0, 100.0, 100.0, 800.0, 600.0),
        ];
        assert!(keys(BAUD, &other_front, Some(BAUD)).is_err());
        assert!(keys(BAUD, &baud_front, Some(OTHER)).is_err());
        assert!(keys(BAUD, &baud_front, None).is_err());
        assert!(keys(BAUD, &[], Some(BAUD)).is_err(), "no window at all");
        let said = keys(BAUD, &other_front, Some(OTHER)).unwrap_err().0;
        assert!(
            said.contains("pid 77's") && said.contains("Other"),
            "{said}"
        );
    }

    /// **A wheel turn goes only to a point in baud's window, in front**:
    /// inside it, yes; outside every window of baud's, no; inside baud's
    /// window but under another application's, no; inside baud's window
    /// behind its own tooltip, yes. Perturbation: walk every window rather
    /// than stopping at the first that is not baud's, and the third case
    /// passes.
    #[test]
    fn a_wheel_turn_goes_only_to_a_point_in_baud_in_front() {
        let list = [
            win(BAUD, 0, 120.0, 90.0, 200.0, 30.0),
            win(BAUD, 0, 100.0, 100.0, 800.0, 600.0),
        ];
        assert_eq!(wheel(BAUD, &list, Some(BAUD), (500.0, 400.0)), Ok(()));
        assert!(wheel(BAUD, &list, Some(BAUD), (50.0, 50.0)).is_err());
        let covered = [
            win(BAUD, 0, 120.0, 90.0, 200.0, 30.0),
            win(OTHER, 0, 400.0, 300.0, 300.0, 300.0),
            win(BAUD, 0, 100.0, 100.0, 800.0, 600.0),
        ];
        assert!(wheel(BAUD, &covered, Some(BAUD), (500.0, 400.0)).is_err());
        assert!(wheel(BAUD, &list, Some(OTHER), (500.0, 400.0)).is_err());
    }

    /// **The window is taken once it has held still where it was first
    /// seen**: readings 50 ms apart wait out the half second, a window that
    /// came and stayed is `Still` at its frame, one that moved 9 points
    /// after it was first seen and stayed there is `Moved` from the first
    /// frame to the second, as the first try's window moved by prompt 1,
    /// and one that is still moving, or gone again, is waited for.
    /// Perturbation: take a window that has held still without comparing
    /// its frame with the first, and the moved window is taken.
    #[test]
    fn the_window_is_taken_once_it_holds_still_where_it_was_first_seen() {
        let at = |x: f64| Rect {
            x,
            y: 161.0,
            w: 786.0,
            h: 572.0,
        };
        let reads = |frames: &[Option<Rect>]| -> Vec<(f64, Option<Rect>)> {
            frames
                .iter()
                .enumerate()
                .map(|(i, f)| (i as f64 * 0.05, *f))
                .collect()
        };
        assert_eq!(settle(&[], 0.5), Settle::Waiting);
        assert_eq!(settle(&reads(&[None, None]), 0.5), Settle::Waiting);
        let mut came = vec![None, None];
        came.extend(std::iter::repeat_n(Some(at(471.0)), 10));
        assert_eq!(settle(&reads(&came), 0.5), Settle::Waiting, "0.45 s");
        came.push(Some(at(471.0)));
        assert_eq!(settle(&reads(&came), 0.5), Settle::Still(at(471.0)));
        let mut moved = vec![Some(at(471.0)); 4];
        moved.extend(std::iter::repeat_n(Some(at(480.0)), 11));
        assert_eq!(
            settle(&reads(&moved), 0.5),
            Settle::Moved {
                first: at(471.0),
                now: at(480.0)
            }
        );
        let mut moving = vec![Some(at(471.0)); 12];
        moving.extend([Some(at(472.0)), Some(at(473.0))]);
        assert_eq!(settle(&reads(&moving), 0.5), Settle::Waiting);
        let mut gone = vec![Some(at(471.0)); 12];
        gone.push(None);
        assert_eq!(settle(&reads(&gone), 0.5), Settle::Waiting);
    }

    /// **The centre is the largest of baud's windows on screen**, not its
    /// tooltip, and nothing of another's.
    #[test]
    fn the_centre_is_baud_s_largest_window() {
        let mut hidden = win(BAUD, 0, 0.0, 0.0, 4000.0, 4000.0);
        hidden.on_screen = OnScreen::No;
        let all = [
            win(BAUD, 0, 120.0, 90.0, 200.0, 30.0),
            win(BAUD, 0, 100.0, 100.0, 800.0, 600.0),
            win(OTHER, 0, 0.0, 0.0, 2000.0, 2000.0),
            hidden,
        ];
        assert_eq!(centre(BAUD, &all), Some((500.0, 400.0)));
        assert_eq!(centre(9, &all), None);
        assert_eq!(
            frame(BAUD, &all),
            Some(Rect {
                x: 100.0,
                y: 100.0,
                w: 800.0,
                h: 600.0
            })
        );
    }
}
