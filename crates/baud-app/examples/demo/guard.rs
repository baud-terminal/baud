//! **The guard that keeps a key or a wheel turn out of every other
//! application**, as pure functions of what the window server and AppKit
//! answer, so a test can hand them any desktop.
//!
//! The keys' rule is the one the README's first recording was typed
//! under: before every key-down and every key-up, the
//! frontmost ordinary window's owner and the frontmost application must
//! both be baud's pid. The wheel's adds `tools/scripts/readme-pointer.swift`'s:
//! the point must lie in a window of baud's with no other process's
//! ordinary window in front of it.
//!
//! Both look only at layer 0, and a lock screen's shield and a screen saver
//! sit above it, so neither can see them. [`console`] looks at the session
//! itself, before the window opens, before every posted event and at the
//! film's end, and an answer it cannot read refuses.

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
/// as `tools/scripts/readme-pointer.swift` passes it over.
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

/// **One thing read from the system.** `Unread` is a read that gave no
/// answer of the kind it should, and [`console`] refuses it as it refuses
/// the state it might be hiding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    Yes,
    No,
    Unread,
}

/// **The session a posted key would land in**, as `mac::console` reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Console {
    /// `IOConsoleLocked` on the IORegistry's root. In no SDK header.
    pub registry_locked: Reading,
    /// `CGSSessionScreenIsLocked` in the session dictionary. In no SDK
    /// header, and absent while the screen is unlocked, so absent is `No`
    /// and only a missing dictionary or a value of another type is `Unread`.
    pub session_locked: Reading,
    /// `kCGSessionOnConsoleKey`, a CFBoolean by
    /// `[[source: macos-sdk-cgsession-h]]`.
    pub on_console: Reading,
    /// `IsSecureEventInputEnabled`, by any process
    /// (`[[source: macos-sdk-carboneventscore-h]]`).
    pub secure_input: Reading,
    /// `CGDisplayIsAsleep` of the main display
    /// (`[[source: macos-sdk-cgdisplayconfiguration-h]]`).
    pub display_asleep: Reading,
    /// `CGDisplayIsActive` of the main display, the same header's.
    pub display_active: Reading,
    /// Whether a process named `ScreenSaverEngine` exists.
    pub saver: Reading,
}

/// **What refuses a key, a window or a film**: each state alone, and each
/// reading that gave no answer. The sentence names the state; the caller
/// says what was not done because of it.
pub fn console(c: &Console) -> Result<(), Refused> {
    const LOCKED: &str = "the screen is locked, so a key would go to its password field";
    let states = [
        (
            c.registry_locked,
            Reading::Yes,
            LOCKED,
            "IOConsoleLocked could not be read, so the screen may be locked and a key would go \
             to its password field",
        ),
        (
            c.session_locked,
            Reading::Yes,
            LOCKED,
            "the window server's session could not be read for a lock, so the screen may be \
             locked and a key would go to its password field",
        ),
        (
            c.on_console,
            Reading::No,
            "this session is not on the console, so a key would not reach baud's window",
            "the window server's session could not be read for the console, so a key might not \
             reach baud's window",
        ),
        (
            c.secure_input,
            Reading::Yes,
            "secure input is on: a password field has the keys, or an application holds it, such \
             as a password manager or a terminal's secure keyboard entry, and posted keys could \
             not reach baud's window reliably",
            "secure input could not be read, so a key might go to a password field",
        ),
        (
            c.display_asleep,
            Reading::Yes,
            "the main display is asleep, so the film would be black",
            "the main display could not be read, so the film might be black",
        ),
        (
            c.display_active,
            Reading::No,
            "the main display is not active, so the film would be black",
            "the main display could not be read, so the film might be black",
        ),
        (
            c.saver,
            Reading::Yes,
            "a screen saver is running, so a key would go to it and the film would show it",
            "the running processes could not be read for a screen saver, so a key might go to it",
        ),
    ];
    for (read, refused, said, unread) in states {
        if read == Reading::Unread {
            return Err(Refused(unread.to_string()));
        }
        if read == refused {
            return Err(Refused(said.to_string()));
        }
    }
    Ok(())
}

/// **The session in a line**, for `--dry`: what [`console`] would act on,
/// and it refuses nothing here.
pub fn describe(c: &Console) -> String {
    let word = |r: Reading, yes: &'static str, no: &'static str, unread: &'static str| match r {
        Reading::Yes => yes,
        Reading::No => no,
        Reading::Unread => unread,
    };
    let lock = match (c.registry_locked, c.session_locked) {
        (Reading::Yes, _) | (_, Reading::Yes) => "locked",
        (Reading::Unread, _) | (_, Reading::Unread) => "lock unreadable",
        _ => "not locked",
    };
    let display = match (c.display_asleep, c.display_active) {
        (Reading::Yes, _) => "display asleep",
        (_, Reading::No) => "display not active",
        (Reading::Unread, _) | (_, Reading::Unread) => "display unreadable",
        _ => "display awake",
    };
    format!(
        "{}, {lock}, {}, {display}, {}",
        word(
            c.on_console,
            "on the console",
            "not on the console",
            "console unreadable"
        ),
        word(
            c.secure_input,
            "secure input on",
            "no secure input",
            "secure input unreadable"
        ),
        word(
            c.saver,
            "a screen saver running",
            "no screen saver",
            "screen saver unreadable"
        ),
    )
}

/// `type` of a Space that a full-screen window has, as `com.apple.spaces`
/// writes it: this Mac's three such read 4, each with an `fs_wid`, and its
/// desktop 0. In no header.
pub const FULL_SCREEN_SPACE: i64 = 4;

/// A display in `com.apple.spaces`'s `Monitors`: its `Display Identifier`
/// and its `Current Space`'s `type`, which only a display in use has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub display: String,
    pub current: Option<i64>,
}

/// The main display's current Space's `type`: the monitor identified `Main`.
pub fn main_space(monitors: &[Monitor]) -> Option<i64> {
    monitors
        .iter()
        .find(|m| m.display == "Main")
        .and_then(|m| m.current)
}

/// **A hint, before the VM**: a window opened from a full-screen Space
/// slides in and [`settle`]'s `Moved` stops the take after the VM started.
/// The preference may be stale until a Space changes, so `Moved` stays the
/// real check, and a preference with no answer refuses nothing.
pub fn space(current: Option<i64>) -> Result<(), Refused> {
    if current == Some(FULL_SCREEN_SPACE) {
        return Err(Refused(
            "the main display is on a full-screen Space, so baud's window would slide in and be \
             refused for moving after the VM started: start from a normal desktop"
                .to_string(),
        ));
    }
    Ok(())
}

/// **`pid`'s frontmost ordinary window on screen**: the first of its
/// windows on layer 0 in `all`, the on-screen list front to back
/// (`kCGWindowListOptionOnScreenOnly`, "ordered from front to back",
/// `[[source: macos-sdk-cgwindow-h]]`), as the keys' guard reads it -- its
/// terminal window rather than a tooltip or a panel, whose layers are
/// above 0. A link's window opens over the window clicked at the same
/// frame, so the two are the same size and only the order tells them
/// apart; the list `kCGWindowListOptionAll` gives has no order the header
/// states.
pub fn frame(pid: i32, all: &[Window]) -> Option<Rect> {
    all.iter()
        .find(|w| w.owner == pid && w.layer == 0 && w.on_screen == OnScreen::Yes)
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

/// **What becomes of the pasteboard when a take ends.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterTake {
    /// Nothing wrote to it, so nothing is owed.
    Untouched,
    /// Its last write is one the film made: what it held goes back.
    FilmsOwn,
    /// Something other than the film wrote to it last: a person's copy,
    /// which stays.
    Theirs,
}

/// **From `changeCount`**: as it was before the take, the last the film
/// knows it made, and as it is now. Only a count still the film's own is the
/// film's to undo.
pub fn after_take(before: isize, own: isize, now: isize) -> AfterTake {
    match (now == own, now == before) {
        (false, _) => AfterTake::Theirs,
        (true, true) => AfterTake::Untouched,
        (true, false) => AfterTake::FilmsOwn,
    }
}

/// **The centre of [`frame`]**, where the wheel turns.
pub fn centre(pid: i32, all: &[Window]) -> Option<(f64, f64)> {
    frame(pid, all).map(|b| (b.x + b.w / 2.0, b.y + b.h / 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The pasteboard goes back only from the film's own count.** A count
    /// past it is someone's copy, whether or not the film also wrote.
    /// Perturbation: restore on any change, and the person's copy is lost.
    #[test]
    fn only_the_films_own_count_is_undone() {
        assert_eq!(after_take(7, 7, 7), AfterTake::Untouched);
        assert_eq!(after_take(7, 8, 8), AfterTake::FilmsOwn);
        assert_eq!(after_take(7, 7, 8), AfterTake::Theirs, "no film copy");
        assert_eq!(after_take(7, 8, 9), AfterTake::Theirs, "after the film's");
    }

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

    /// The session with nothing wrong in it.
    const CLEAR: Console = Console {
        registry_locked: Reading::No,
        session_locked: Reading::No,
        on_console: Reading::Yes,
        secure_input: Reading::No,
        display_asleep: Reading::No,
        display_active: Reading::Yes,
        saver: Reading::No,
    };

    /// **Each state alone refuses, in a sentence naming it, and all clear
    /// passes**, the absent lock key among what passes: this table's first
    /// row is the session as an unlocked Mac reads it. Perturbation: drop a
    /// row of [`console`]'s and its state passes.
    #[test]
    fn each_state_alone_refuses_and_all_clear_passes() {
        assert_eq!(console(&CLEAR), Ok(()));
        let rows = [
            (
                Console {
                    registry_locked: Reading::Yes,
                    ..CLEAR
                },
                "the screen is locked",
            ),
            (
                Console {
                    session_locked: Reading::Yes,
                    ..CLEAR
                },
                "the screen is locked",
            ),
            (
                Console {
                    on_console: Reading::No,
                    ..CLEAR
                },
                "not on the console",
            ),
            (
                Console {
                    secure_input: Reading::Yes,
                    ..CLEAR
                },
                "secure input is on",
            ),
            (
                Console {
                    display_asleep: Reading::Yes,
                    ..CLEAR
                },
                "is asleep",
            ),
            (
                Console {
                    display_active: Reading::No,
                    ..CLEAR
                },
                "not active",
            ),
            (
                Console {
                    saver: Reading::Yes,
                    ..CLEAR
                },
                "a screen saver is running",
            ),
        ];
        for (c, says) in rows {
            let why = console(&c).expect_err(says).0;
            assert!(why.contains(says), "{why}");
        }
    }

    /// **A reading that gave no answer refuses, every one of them**: the
    /// branch a missing dictionary, an absent `IOConsoleLocked` or a value
    /// of another type takes. Perturbation: refuse only `Yes`, and all seven
    /// pass.
    #[test]
    fn an_unreadable_state_refuses() {
        let set: [fn(&mut Console); 7] = [
            |c| c.registry_locked = Reading::Unread,
            |c| c.session_locked = Reading::Unread,
            |c| c.on_console = Reading::Unread,
            |c| c.secure_input = Reading::Unread,
            |c| c.display_asleep = Reading::Unread,
            |c| c.display_active = Reading::Unread,
            |c| c.saver = Reading::Unread,
        ];
        for s in set {
            let mut c = CLEAR;
            s(&mut c);
            assert!(console(&c).is_err(), "{c:?}");
        }
    }

    /// **`--dry`'s line says the state**: all clear in the words the record
    /// gives it, and a locked or unreadable session says so rather than
    /// reading as clear. Perturbation: read only `registry_locked` for the
    /// lock and the second case says not locked.
    #[test]
    fn the_dry_line_says_the_state() {
        assert_eq!(
            describe(&CLEAR),
            "on the console, not locked, no secure input, display awake, no screen saver"
        );
        let locked = Console {
            session_locked: Reading::Yes,
            ..CLEAR
        };
        assert!(describe(&locked).contains(", locked,"));
        let unread = Console {
            registry_locked: Reading::Unread,
            ..CLEAR
        };
        assert!(describe(&unread).contains("lock unreadable"));
        let asleep = Console {
            display_asleep: Reading::Yes,
            display_active: Reading::No,
            saver: Reading::Yes,
            secure_input: Reading::Yes,
            on_console: Reading::No,
            ..CLEAR
        };
        assert_eq!(
            describe(&asleep),
            "not on the console, not locked, secure input on, display asleep, \
             a screen saver running"
        );
    }

    /// **The main display's Space, not any display's and not its list**: the
    /// monitor identified `Main` on a full-screen Space refuses, on a desktop
    /// passes whatever else exists, and a preference with no answer refuses
    /// nothing, this being a hint. Perturbation: match any monitor and the
    /// second case refuses.
    #[test]
    fn only_the_main_display_on_a_full_screen_space_refuses() {
        let m = |d: &str, c| Monitor {
            display: d.to_string(),
            current: c,
        };
        let full = [m("Main", Some(FULL_SCREEN_SPACE))];
        assert_eq!(main_space(&full), Some(FULL_SCREEN_SPACE));
        assert!(
            space(main_space(&full))
                .unwrap_err()
                .0
                .contains("normal desktop")
        );
        let desk = [m("Main", Some(0)), m("OTHER", Some(FULL_SCREEN_SPACE))];
        assert_eq!(space(main_space(&desk)), Ok(()));
        assert_eq!(space(main_space(&[m("Main", None)])), Ok(()));
        assert_eq!(space(main_space(&[m("OTHER", Some(4))])), Ok(()));
        assert_eq!(space(main_space(&[])), Ok(()));
    }

    /// **The centre is baud's frontmost ordinary window on screen**, not
    /// its tooltip above it (layer 3), not one off screen, and nothing of
    /// another's in front.
    #[test]
    fn the_centre_is_baud_s_frontmost_window() {
        let mut hidden = win(BAUD, 0, 0.0, 0.0, 4000.0, 4000.0);
        hidden.on_screen = OnScreen::No;
        let all = [
            win(OTHER, 0, 0.0, 0.0, 2000.0, 2000.0),
            win(BAUD, 3, 120.0, 90.0, 200.0, 30.0),
            hidden,
            win(BAUD, 0, 100.0, 100.0, 800.0, 600.0),
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

    /// **Of two windows of baud's the same size, the front one is the
    /// frame**: a link's window opened over the window clicked, at its
    /// size, comes first in the list front to back and wins, wherever each
    /// lies; with the two the other way round, the other wins. The size
    /// cannot tell them apart. Perturbation: pick the largest, and one of
    /// the two orders gives the window behind.
    #[test]
    fn of_two_equal_windows_the_front_one_is_the_frame() {
        let a = win(BAUD, 0, 100.0, 100.0, 800.0, 600.0);
        let b = win(BAUD, 0, 140.0, 130.0, 800.0, 600.0);
        assert_eq!(frame(BAUD, &[a.clone(), b.clone()]), Some(a.bounds));
        assert_eq!(frame(BAUD, &[b.clone(), a.clone()]), Some(b.bounds));
    }
}
