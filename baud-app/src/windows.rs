//! **The open windows, and what a window's close leaves.**
//!
//! The macOS app runs any number of windows in one process, each with
//! its own shell, terminal, renderer and timers: Command-N opens one,
//! Command-W or the close button closes one and hangs up its shell, and
//! Command-Q quits them all. The HIG gives Command-N to "Open a new
//! document" and Command-W to "Close the active window"
//! (`[[source: apple-hig-keyboards]]`), and its menu bar puts both in the
//! File menu (`[[source: apple-hig-the-menu-bar]]`).
//!
//! **The app stays running when its last window closes**
//! ([`LAST_WINDOW`]): the window crate's File menu has New Window and its
//! application delegate the Dock's reopen, and both reach the app with no
//! window open ([`Asked`] is the hand-off to a window that is). That is
//! AppKit's answer when no delegate says otherwise: `NO` from
//! `applicationShouldTerminateAfterLastWindowClosed:`, and "control
//! returns to the main event loop and the application is not terminated"
//! (`[[source: apple-nsapplicationdelegate-terminate-after-last-window-closed]]`).
//! Ghostty's `quit-after-last-window-closed` is false on macOS "since that
//! is standard behavior for a macOS application" and true on Linux
//! (`[[source: ghostty-config-d9840f3]]`), kitty "will stay running,
//! even with no open windows, as is the expected behavior on macOS"
//! (`[[source: kitty-options-definition]]`), and iTerm2's
//! `QuitWhenAllWindowsClosed` defaults to `NO`
//! (`[[source: iterm2-269f725-application-delegate]]`,
//! `[[source: iterm2-269f725-preferences]]`).
//!
//! On Linux a new window is a new process, as foot's `spawn-terminal`
//! (Control+Shift+n) starts one in the current directory
//! (`[[source: foot-2705e36-foot-ini-5]]`, `term_spawn_new()` in
//! `[[source: foot-terminal-cb27717]]`): each window is its own
//! `xdg_toplevel`, and its process exits when its window closes.
//!
//! **A Command-N window is a window of its own, never merged into a tab
//! group**, whatever the system's "Prefer tabs when opening documents"
//! says: the window crate builds every window with
//! `NSWindowTabbingModeDisallowed`, "The window explicitly should not
//! prefer to tab when shown", because a tab bar "on top of the material
//! ... undoes the whole effect", and the app opts out of AppKit's
//! automatic tabbing as a whole before its first window: "applications
//! can explicitly opt out of all automatic tabbing by setting it to NO"
//! (`[[source: sdk27-nswindow-h]]` 262-271, 686-688). Only a window left
//! at `NSWindowTabbingModeAutomatic` follows the system's preference, and
//! baud has no tabs of its own for one to join.
//!
//! **Each window's notifications are its own**: their identifiers carry
//! the window's key ([`WindowId::notice_prefix`]), so two windows' first
//! posts do not replace each other, and a click goes to the window that
//! posted it, or nowhere once it has closed ([`clicked`]). **The Dock
//! tile is the app's**, one for every window, and shows the most urgent
//! window's progress (`notify::DockBars`).
//!
//! **A new window starts in the directory of the window it was opened
//! from**: the one its shell's `OSC 7` names on this machine, else the one
//! baud reads for the title, else where a launch starts. Ghostty's
//! `window-inherit-working-directory` defaults to that, and foot's
//! `spawn-terminal` does it.
#![forbid(unsafe_code)]

/// A window's key in the set, never reused while the process runs.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowId(u64);

#[cfg(any(target_os = "macos", test))]
impl WindowId {
    const FIRST: WindowId = WindowId(0);

    /// The key the next window opened gets.
    fn after(self) -> WindowId {
        WindowId(self.0 + 1)
    }

    /// **What begins the identifier of every notification this window
    /// posts**: the process's `base` and the window's key. Each window's
    /// terminal numbers its own notifications from the first, so without
    /// the key two windows' first posts had one identifier, and the
    /// second replaced the first in Notification Center.
    pub fn notice_prefix(self, base: &str) -> String {
        format!("{base}-w{}", self.0)
    }
}

/// **The window that posted the notification `id` names**, read back
/// from its [`WindowId::notice_prefix`]: `None` for an identifier no
/// window of this process made.
#[cfg(any(target_os = "macos", test))]
pub fn notice_poster(id: &str, base: &str) -> Option<WindowId> {
    let (key, _) = id.strip_prefix(base)?.strip_prefix("-w")?.split_once('-')?;
    key.parse().ok().map(WindowId)
}

/// Whose a notification's click is, for the window taking its turn.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clicked {
    /// This window posted it.
    This,
    /// Another window that is open did.
    Another,
    /// No window that is open did.
    Closed,
}

/// **Whose the click on notification `id` is** for window `me`, with
/// `open` the windows open: the click goes to the window that posted it,
/// and one whose window has closed goes nowhere.
#[cfg(any(target_os = "macos", test))]
pub fn clicked(id: &str, base: &str, me: WindowId, open: &[WindowId]) -> Clicked {
    match notice_poster(id, base) {
        Some(w) if w == me => Clicked::This,
        Some(w) if open.contains(&w) => Clicked::Another,
        _ => Clicked::Closed,
    }
}

/// What a close leaves open.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Left {
    /// This many windows are still open.
    Others(usize),
    /// The window closed was the last.
    None,
}

/// **What the app does when its last window closes.** One of the two is
/// chosen ([`LAST_WINDOW`]), so the app never builds the other.
#[cfg(any(target_os = "macos", test))]
#[allow(
    dead_code,
    reason = "LAST_WINDOW chooses one; Quit is what the app did before File > New Window"
)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LastWindow {
    /// The app quits, as it did when its one window's shell exited.
    Quit,
    /// The app keeps running with no window, as AppKit does with no
    /// delegate, and File > New Window or a Dock click opens one.
    StayRunning,
}

/// **The last window's close leaves the app running.** A Mac app that
/// stays running with no window opens one again from File > New Window
/// or a click on its Dock icon
/// (`[[source: apple-nsapplicationdelegate-should-handle-reopen]]`). The
/// window crate offers both since 1.0.0-beta.7, its File menu and an
/// application delegate whose `newWindow:` and
/// `applicationShouldHandleReopen:` reach the app with no window open, and
/// the app opens a window from either. [`LastWindow::Quit`] is what the
/// app did before it had more than one window.
#[cfg(any(target_os = "macos", test))]
pub const LAST_WINDOW: LastWindow = LastWindow::StayRunning;

/// What the app does after a window's close.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfterClose {
    /// The app goes on running.
    Run,
    /// The app quits.
    Quit,
}

/// **What a close leaves the app doing**: it goes on while another
/// window is open, and when none is, `last` says.
#[cfg(any(target_os = "macos", test))]
pub fn after_close(left: Left, last: LastWindow) -> AfterClose {
    match (left, last) {
        (Left::Others(_), _) | (Left::None, LastWindow::StayRunning) => AfterClose::Run,
        (Left::None, LastWindow::Quit) => AfterClose::Quit,
    }
}

/// **The open windows, each owning what it was opened with**: a
/// window's shell, terminal and renderer are its own, so a close hands
/// back that window's and touches no other.
#[cfg(any(target_os = "macos", test))]
pub struct Windows<W> {
    open: Vec<(WindowId, W)>,
    next: WindowId,
}

#[cfg(any(target_os = "macos", test))]
impl<W> Windows<W> {
    pub const fn new() -> Self {
        Windows {
            open: Vec::new(),
            next: WindowId::FIRST,
        }
    }

    /// Adds a window, under a key no other window has had.
    pub fn open(&mut self, w: W) -> WindowId {
        let id = self.next;
        self.next = id.after();
        self.open.push((id, w));
        id
    }

    /// Takes the window out of the set and hands it back, with what is
    /// left open; `None` when it is not open, which a second close of one
    /// window finds.
    pub fn close(&mut self, id: WindowId) -> Option<(W, Left)> {
        let at = self.open.iter().position(|(i, _)| *i == id)?;
        let (_, w) = self.open.remove(at);
        let left = match self.open.len() {
            0 => Left::None,
            n => Left::Others(n),
        };
        Some((w, left))
    }

    pub fn get(&self, id: WindowId) -> Option<&W> {
        self.open.iter().find(|(i, _)| *i == id).map(|(_, w)| w)
    }

    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut W> {
        self.open.iter_mut().find(|(i, _)| *i == id).map(|(_, w)| w)
    }

    /// The keys of the windows open, in the order they opened.
    pub fn ids(&self) -> Vec<WindowId> {
        self.open.iter().map(|(i, _)| *i).collect()
    }
}

/// **Where a new window goes**: over the window a link's window was asked
/// from, at exactly its frame, covering the window it came from -- or
/// else a step down and right of the key window, as AppKit cascades one,
/// or where AppKit puts a window when none is open.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Placed<R> {
    Over(R),
    Cascade(R),
    Default,
}

/// [`Placed`] for a window asked `over` a window's frame, or with `key`
/// the key window's frame when it was asked.
#[cfg(any(target_os = "macos", test))]
pub fn placed<R>(over: Option<R>, key: Option<R>) -> Placed<R> {
    match (over, key) {
        (Some(frame), _) => Placed::Over(frame),
        (None, Some(frame)) => Placed::Cascade(frame),
        (None, None) => Placed::Default,
    }
}

/// **The window a closing link's window hands the keyboard back to**: the
/// one it was opened over, if that is still open; none when it closed
/// first, so focus never goes to a window that is gone.
#[cfg(any(target_os = "macos", test))]
pub fn focus_back<W>(open: &Windows<W>, over: Option<WindowId>) -> Option<&W> {
    open.get(over?)
}

/// What a key chord asks of the windows.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chord {
    /// Open a new window.
    New,
}

/// Whether Command was held alone, or with Shift, Option or Control.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Alone,
    WithOthers,
}

/// **The window chord a Command key names on macOS**: Command-N alone
/// opens a window, where the File menu's New Window did not take it.
/// Command-W and Command-Q are the menu's own (`Close` and `Quit`, which
/// AppKit performs), so they never reach the view.
#[cfg(any(target_os = "macos", test))]
pub fn mac_chord(characters: &str, held: Command) -> Option<Chord> {
    match (characters, held) {
        ("n", Command::Alone) => Some(Chord::New),
        _ => None,
    }
}

/// **A New Window asked of one window, until its driver takes it**: the
/// File menu's New Window reaches the app delegate inside the menu's
/// action, and the window it was asked from opens one on its view's next
/// wake, outside that action, as its own Command-N does.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Default)]
pub struct Asked(std::cell::Cell<Option<WindowId>>);

#[cfg(any(target_os = "macos", test))]
impl Asked {
    pub const fn new() -> Asked {
        Asked(std::cell::Cell::new(None))
    }

    /// A New Window asked of `of`, in place of any asked before.
    pub fn ask(&self, of: WindowId) {
        self.0.set(Some(of));
    }

    /// Whether the window `me` was asked, the ask taken with the answer,
    /// so one ask opens one window.
    pub fn take(&self, me: Option<WindowId>) -> bool {
        let asked = me.is_some() && self.0.get() == me;
        if asked {
            self.0.set(None);
        }
        asked
    }
}

/// **The directory a new window starts in**: the path the window's
/// `OSC 7` names on this machine (`title::local`), else the directory
/// read for its title, else `None`, where a launch starts. Only an
/// absolute path is taken: a relative one would be read against wherever
/// the new shell is.
pub fn inherited(
    cwd: Option<baud_core::WorkingDirectory<'_>>,
    hosts: &[String],
    read: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    cwd.and_then(|d| crate::title::local(d, hosts))
        .map(|p| std::path::PathBuf::from(std::ffi::OsStr::from_bytes(p)))
        .or_else(|| read.map(std::path::Path::to_path_buf))
        .filter(|p| p.is_absolute())
}

/// **The variable that says which screen a window opens on**, read
/// once per window before it is shown ([`screen`]). Unset, a window is
/// centred by AppKit's `center`, on the screen with the key window
/// (`mainScreen`, "Screen with key window", `[[source: sdk27-nsscreen-h]]`).
/// `primary` centres it on the primary screen, the one with the menu bar,
/// first in `NSScreen.screens` and with its origin at (0, 0)
/// (`[[source: apple-nsscreen-screens]]`): the display CoreGraphics names
/// its main display, which the README film's recorder films
/// (`baud-app/examples/demo`). Like `BAUD_GRID`, it is for a run that
/// must open where it is filmed or measured.
#[cfg(any(target_os = "macos", test))]
pub const SCREEN: &str = "BAUD_SCREEN";

/// Which screen a window opens on.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// The screen with the key window, where AppKit centres it.
    KeyWindow,
    /// The primary screen.
    Primary,
}

/// [`Screen`], from [`SCREEN`] in `env`; a value other than `primary` is
/// refused in words, and the window opens where it would unset.
#[cfg(any(target_os = "macos", test))]
pub fn screen(env: Option<&std::ffi::OsStr>) -> Result<Screen, String> {
    match env {
        None => Ok(Screen::KeyWindow),
        Some(v) if v == "primary" => Ok(Screen::Primary),
        Some(v) => Err(format!(
            "{SCREEN}={v:?} is not primary; the window opens on the screen with the key window"
        )),
    }
}

/// **The origin of a window `size` points big centred in `visible`**,
/// both in AppKit's screen points, origin at the bottom left, on whole
/// points: a window taller than the area keeps its top, and the titlebar
/// with it, inside the area, and one wider keeps its left edge.
#[cfg(any(target_os = "macos", test))]
pub fn centred(size: (f64, f64), visible: (f64, f64, f64, f64)) -> (f64, f64) {
    let (w, h) = size;
    let (x, y, vw, vh) = visible;
    let left = x + ((vw - w) / 2.0).max(0.0);
    let bottom = y + vh - h - ((vh - h) / 2.0).max(0.0);
    (left.round(), bottom.round())
}

/// **A new window on Linux: a new baud**, as foot's `spawn-terminal`
/// execs a new foot in the terminal's directory. The same executable,
/// with the environment this one has -- so the same config -- but for
/// `WAYLAND_SOCKET`, started in `dir` with `PWD` naming it (XBD 8's `PWD`,
/// `[[source: posix-xbd-8]]`);
/// its own launch rule then keeps the shell there, and it plays the
/// logo as every window does (`splash.rs`). The child is waited for on
/// a thread of its own, so it never stays a zombie, and its exit says
/// nothing to this window.
#[cfg(target_os = "linux")]
pub fn spawn_window(dir: Option<&std::path::Path>) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            crate::warn!("new window: this program's path cannot be read: {e}");
            return;
        }
    };
    let mut cmd = new_baud(&exe, dir);
    match cmd.spawn() {
        Ok(mut child) => {
            let spawned = std::thread::Builder::new()
                .name("baud new window".into())
                .spawn(move || {
                    let _ = child.wait();
                });
            if let Err(e) = spawned {
                crate::warn!("new window: no thread to wait for it: {e}");
            }
        }
        Err(e) => crate::warn!("new window: baud did not start: {e}"),
    }
}

/// **A new baud, `exe`, started in `dir`**: [`spawn_window`]'s and
/// [`running_window`]'s one start.
#[cfg(any(target_os = "linux", test))]
fn new_baud(exe: &std::path::Path, dir: Option<&std::path::Path>) -> std::process::Command {
    let mut cmd = std::process::Command::new(exe);
    // A directory that has gone since would fail the spawn, so the new
    // window starts where a launch starts instead.
    if let Some(d) = dir.filter(|d| d.is_dir()) {
        cmd.current_dir(d).env("PWD", d);
    }
    // The socket number a compositor handed this process names nothing of
    // the child's: libwayland unsets it once taken (`[[source:
    // libwayland-93c04ad-client]]` 1379), and the child connects by
    // `WAYLAND_DISPLAY`.
    cmd.env_remove("WAYLAND_SOCKET");
    cmd
}

/// **The new baud a link's `action = "window"` starts on Linux**, whose
/// driver runs one window per process: `exe` -- this program, as
/// `current_exe` reads it -- with `--grid` and `size`, the clicking
/// window's grid, then `-e` and `run`'s elements, each one argument and
/// none re-split (`run_arg.rs`), started in `dir` as New Window starts one
/// ([`new_baud`]), with no shell between. Started through the opener on
/// the turn after the click, and reaped by it.
#[cfg(any(target_os = "linux", test))]
pub fn running_window(
    exe: &std::path::Path,
    run: crate::rules::Run,
    size: Option<crate::run_arg::Grid>,
    dir: Option<&std::path::Path>,
) -> std::process::Command {
    let mut cmd = new_baud(exe, dir);
    cmd.args(crate::run_arg::args(run, size));
    cmd
}

/// **The grid of the window a link was clicked in**, `term`'s: what a
/// Linux link's window opens at ([`link_window`]).
#[cfg(any(target_os = "linux", test))]
pub fn grid_of(term: &baud_core::Term<'_>) -> crate::run_arg::Grid {
    let (cols, rows) = term.geometry();
    crate::run_arg::Grid::of(cols, rows)
}

/// **What the Linux driver's `Later::Window` starts**: [`running_window`]
/// at `grid`, the clicked window's ([`grid_of`]), always given, so the new
/// window opens at the grid the link was clicked in and not the default.
#[cfg(any(target_os = "linux", test))]
pub fn link_window(
    exe: &std::path::Path,
    run: crate::rules::Run,
    dir: Option<&std::path::Path>,
    grid: crate::run_arg::Grid,
) -> std::process::Command {
    running_window(exe, run, Some(grid), dir)
}

#[cfg(test)]
mod tests {
    use super::{
        AfterClose, Asked, Chord, Clicked, Command, LAST_WINDOW, LastWindow, Left, Placed, Screen,
        Windows, after_close, centred, clicked, focus_back, inherited, mac_chord, placed, screen,
    };
    use baud_core::{FileHost, Owned, Term, WorkingDirectory};
    use baud_domain::{BytePos, ReadLen, Row, WinSize};
    use std::path::{Path, PathBuf};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 4,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    fn first_row(t: &Term<'_>) -> String {
        t.row(Row::from_index(0))
            .expect("row 0")
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or(' ')
            })
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// **Three windows, each with its own terminal, and closing one leaves
    /// the other two as they were.** Each terminal is fed its own text
    /// after all three are open, so a set that kept one terminal for every
    /// window, or replaced the last, shows another window's text or none.
    #[test]
    fn closing_one_of_three_windows_leaves_the_other_two_their_own_terminals() {
        let mut stores: Vec<Owned> = (0..3).map(|_| Owned::new(&WS)).collect();
        let mut set = Windows::new();
        let ids: Vec<_> = stores
            .iter_mut()
            .map(|o| set.open(Term::new(o.storage(), &WS)))
            .collect();
        assert!(
            ids.iter().all(|id| set.get(*id).is_some()),
            "three windows open"
        );
        for (id, text) in ids.iter().zip([&b"one"[..], b"two", b"three"]) {
            feed(set.get_mut(*id).expect("open"), text);
        }
        let (closed, left) = set.close(ids[1]).expect("the second was open");
        assert_eq!(
            first_row(&closed),
            "two",
            "the closed window's own terminal"
        );
        assert_eq!(left, Left::Others(2));
        assert!(set.get(ids[1]).is_none(), "closed stays closed");
        assert_eq!(first_row(set.get(ids[0]).expect("first")), "one");
        assert_eq!(first_row(set.get(ids[2]).expect("third")), "three");
        assert!(set.close(ids[1]).is_none(), "a second close finds nothing");
        let (_, left) = set.close(ids[0]).expect("first");
        assert_eq!(left, Left::Others(1));
        let (last, left) = set.close(ids[2]).expect("third");
        assert_eq!(first_row(&last), "three");
        assert_eq!(left, Left::None, "the last window's close leaves none");
        // A window opened after the last closed gets a key no closed one
        // had, so a late close of an old key cannot take it.
        let mut store = Owned::new(&WS);
        let again = set.open(Term::new(store.storage(), &WS));
        assert!(!ids.contains(&again), "keys are not reused");
        assert!(set.close(ids[2]).is_none());
        assert!(set.get(again).is_some());
        let (_, left) = set.close(again).expect("the new one");
        assert_eq!(left, Left::None, "it was the only one open");
    }

    /// **Closing one of two windows leaves the app running, and so does
    /// closing the last**, now that File > New Window and a Dock click
    /// open a window with none open (`LAST_WINDOW`). The control is the
    /// other choice: told to quit, the last close quits the app, so the
    /// app runs on from the choice and not from the close. Perturbation:
    /// choose to quit and the last close quits it.
    #[test]
    fn closing_the_last_window_or_one_of_two_leaves_the_app_running() {
        let mut stores: Vec<Owned> = (0..2).map(|_| Owned::new(&WS)).collect();
        let mut set = Windows::new();
        let ids: Vec<_> = stores
            .iter_mut()
            .map(|o| set.open(Term::new(o.storage(), &WS)))
            .collect();
        let (_, left) = set.close(ids[0]).expect("the first was open");
        assert_eq!(
            after_close(left, LAST_WINDOW),
            AfterClose::Run,
            "one of two closed: the other goes on"
        );
        let (_, left) = set.close(ids[1]).expect("the second was open");
        assert_eq!(
            after_close(left, LastWindow::Quit),
            AfterClose::Quit,
            "the control: told to quit, the last close quits"
        );
        assert_eq!(
            after_close(left, LAST_WINDOW),
            AfterClose::Run,
            "the last window closed: the app runs on"
        );
    }

    /// **A notification's click goes to the window that posted it**: two
    /// windows' first notifications, one token each, have two identifiers,
    /// each read back to its own window; a click on one is the poster's
    /// and not the other's, and once its window has closed it is nobody's.
    /// An identifier another process or none made is nobody's either.
    #[test]
    fn a_notifications_click_goes_to_the_window_that_posted_it() {
        let mut stores: Vec<Owned> = (0..3).map(|_| Owned::new(&WS)).collect();
        let mut set = Windows::new();
        let ids: Vec<_> = stores
            .iter_mut()
            .map(|o| set.open(Term::new(o.storage(), &WS)))
            .collect();
        let base = "baud-41";
        let token = "NoticeToken(0)";
        let [a, b, c] =
            [ids[0], ids[1], ids[2]].map(|w| format!("{}-{token}", w.notice_prefix(base)));
        assert_ne!(a, b, "one token, two windows, two identifiers");
        let open = set.ids();
        assert_eq!(clicked(&a, base, ids[0], &open), Clicked::This);
        assert_eq!(clicked(&a, base, ids[1], &open), Clicked::Another);
        assert_eq!(clicked(&b, base, ids[1], &open), Clicked::This);
        assert_eq!(clicked(&b, base, ids[0], &open), Clicked::Another);
        let _ = set.close(ids[2]);
        let open = set.ids();
        assert_eq!(clicked(&c, base, ids[0], &open), Clicked::Closed);
        assert_eq!(clicked(&c, base, ids[1], &open), Clicked::Closed);
        for other in [
            format!("baud-42-w0-{token}"),
            "baud-41".into(),
            "nonsense".into(),
        ] {
            assert_eq!(
                clicked(&other, base, ids[0], &open),
                Clicked::Closed,
                "{other}"
            );
        }
    }

    /// **`BAUD_SCREEN=primary` opens the window on the primary screen**,
    /// unset on the key window's, and any other value is refused in words
    /// that name it.
    #[test]
    fn the_screen_variable_names_the_primary_screen_or_nothing() {
        let os = std::ffi::OsStr::new;
        assert_eq!(screen(None), Ok(Screen::KeyWindow));
        assert_eq!(screen(Some(os("primary"))), Ok(Screen::Primary));
        let said = screen(Some(os("main"))).unwrap_err();
        assert!(said.contains("BAUD_SCREEN=\"main\""), "{said}");
    }

    /// **A window is centred in the area, on whole points**: the README
    /// film's 786x572 window in a 1728x1085 area above a 32-point Dock
    /// opens at 471,289; one taller than the area keeps its top at the
    /// area's top, and one wider its left edge at the area's left, so the
    /// titlebar is never off the screen. A display left of the primary has
    /// a negative x. Perturbation: centre without the floor at zero and
    /// the tall window's top passes the area's.
    #[test]
    fn a_window_is_centred_in_the_area_its_top_kept_inside() {
        assert_eq!(
            centred((786.0, 572.0), (0.0, 32.0, 1728.0, 1085.0)),
            (471.0, 289.0)
        );
        let (x, y) = centred((786.0, 1300.0), (0.0, 32.0, 1728.0, 1085.0));
        assert_eq!((x, y + 1300.0), (471.0, 1117.0));
        assert_eq!(centred((2000.0, 572.0), (0.0, 0.0, 1728.0, 1085.0)).0, 0.0);
        assert_eq!(
            centred((786.0, 572.0), (-1920.0, 0.0, 1920.0, 1055.0)),
            (-1353.0, 242.0)
        );
        assert_eq!(
            centred((785.0, 571.0), (0.0, 0.0, 1728.0, 1085.0)),
            (472.0, 257.0)
        );
    }

    /// Command-N alone opens a window; with another modifier, or another
    /// key, nothing.
    #[test]
    fn command_n_alone_opens_a_window() {
        assert_eq!(mac_chord("n", Command::Alone), Some(Chord::New));
        assert_eq!(mac_chord("n", Command::WithOthers), None);
        assert_eq!(mac_chord("w", Command::Alone), None);
        assert_eq!(mac_chord("N", Command::Alone), None);
    }

    /// **A New Window asked of one window is opened by it, once**: the
    /// other window's wake, or a driver not yet given its key, takes
    /// nothing, and the window asked takes it on its first wake and not
    /// again. Perturbations: answer any window and the other's wake opens
    /// it; keep the ask after it is taken and the second wake opens a
    /// second window.
    #[test]
    fn a_new_window_asked_of_one_window_is_opened_by_it_once() {
        let mut w = Windows::new();
        let (a, b) = (w.open(()), w.open(()));
        let asked = Asked::new();
        assert!(!asked.take(Some(a)), "nothing asked");
        asked.ask(a);
        assert!(!asked.take(Some(b)), "another window");
        assert!(!asked.take(None), "a driver not yet given its key");
        assert!(asked.take(Some(a)), "the window asked");
        assert!(!asked.take(Some(a)), "once");
        asked.ask(a);
        asked.ask(b);
        assert!(!asked.take(Some(a)), "the later ask replaces it");
        assert!(asked.take(Some(b)));
    }

    /// The `OSC 7` path wins over the directory read for the title, a
    /// report naming another host is not taken, and a relative path is
    /// never inherited.
    #[test]
    fn a_new_window_inherits_the_reported_directory_then_the_read_one() {
        let hosts = vec!["here".to_string()];
        let local = WorkingDirectory {
            host: FileHost::Local,
            path: b"/srv/reported",
        };
        let named = WorkingDirectory {
            host: FileHost::Named(b"HERE"),
            path: b"/srv/named",
        };
        let other = WorkingDirectory {
            host: FileHost::Named(b"elsewhere"),
            path: b"/srv/other",
        };
        let read = Path::new("/srv/read");
        assert_eq!(
            inherited(Some(local), &hosts, Some(read)),
            Some(PathBuf::from("/srv/reported"))
        );
        assert_eq!(
            inherited(Some(named), &hosts, None),
            Some(PathBuf::from("/srv/named"))
        );
        assert_eq!(
            inherited(Some(other), &hosts, Some(read)),
            Some(PathBuf::from("/srv/read"))
        );
        assert_eq!(inherited(None, &hosts, None), None);
        assert_eq!(inherited(None, &hosts, Some(Path::new("rel"))), None);
    }

    /// **A link's window on Linux is this program again, `-e` and the
    /// argv after it, with no shell**: the program the executable's path,
    /// the arguments `--grid` and the clicking window's grid, `-e`, then each
    /// element as it was -- a space and a
    /// `--` inside one included -- which `run_arg::asked` reads back to the
    /// same argv, started in the folder given with `PWD` naming it, and
    /// `WAYLAND_SOCKET` left out; a folder that is gone starts it where a
    /// launch would. Named breaks: join the elements into one argument --
    /// the arguments are four, not six; leave the grid out -- the new window
    /// opens at the default grid.
    #[test]
    fn a_links_window_on_linux_is_this_program_with_dash_e() {
        use std::ffi::{OsStr, OsString};
        let argv: Vec<OsString> = ["hx", "/tmp/a b.rs", "--"]
            .iter()
            .map(OsString::from)
            .collect();
        let run = crate::rules::Run::given(argv).expect("a program");
        let dir = std::env::temp_dir();
        let ws = crate::run_arg::Grid::of(
            baud_domain::Cols::of_index(132),
            baud_domain::Rows::of_index(43),
        );
        let cmd = super::running_window(
            Path::new("/opt/baud/bin/baud"),
            run.clone(),
            Some(ws),
            Some(&dir),
        );
        assert_eq!(cmd.get_program(), "/opt/baud/bin/baud");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, ["--grid", "132x43", "-e", "hx", "/tmp/a b.rs", "--"]);
        assert_eq!(
            crate::run_arg::asked(args.iter().map(OsString::from), None),
            crate::run_arg::Asked::Run(run.clone(), Some(ws)),
            "the new process reads the argv it was given"
        );
        assert_eq!(cmd.get_current_dir(), Some(dir.as_path()));
        let envs: Vec<(&OsStr, Option<&OsStr>)> = cmd.get_envs().collect();
        assert!(
            envs.contains(&(OsStr::new("PWD"), Some(dir.as_os_str()))),
            "{envs:?}"
        );
        assert!(
            envs.contains(&(OsStr::new("WAYLAND_SOCKET"), None)),
            "{envs:?}"
        );
        let gone =
            super::running_window(Path::new("/b"), run, None, Some(Path::new("/no/such/dir")));
        assert_eq!(gone.get_current_dir(), None, "a folder that is gone");
    }

    /// **A Linux link's window opens at the grid of the window clicked in**:
    /// a terminal of 132x43 gives `--grid 132x43` ahead of `-e` in what
    /// `Later::Window` starts, and the new process reads that grid back. A
    /// grid of 80x24, the control, gives `80x24`. Named break: leave the
    /// grid out of `link_window` -- no `--grid`, and the new window opens at
    /// the default grid.
    #[test]
    fn a_linux_links_window_carries_the_clicked_windows_grid() {
        use std::ffi::{OsStr, OsString};
        let argv: Vec<OsString> = ["hx", "/tmp/x.rs"].iter().map(OsString::from).collect();
        let run = crate::rules::Run::given(argv).expect("a program");
        for (cols, rows, want) in [(132, 43, "132x43"), (80, 24, "80x24")] {
            let ws = WinSize {
                ws_col: cols,
                ws_row: rows,
            };
            let mut owned = Owned::new(&ws);
            let t = Term::new(owned.storage(), &ws);
            let grid = super::grid_of(&t);
            let cmd = super::link_window(Path::new("/opt/baud/bin/baud"), run.clone(), None, grid);
            let args: Vec<&OsStr> = cmd.get_args().collect();
            assert_eq!(args, ["--grid", want, "-e", "hx", "/tmp/x.rs"], "{want}");
            assert_eq!(
                crate::run_arg::asked(args.iter().map(OsString::from), None),
                crate::run_arg::Asked::Run(run.clone(), Some(grid)),
                "the new process reads the grid back"
            );
        }
    }

    /// **A link's window takes the clicked window's frame exactly**, over
    /// the key window's cascade; with no window to cover it cascades from
    /// the key window, and with neither it goes where AppKit puts one.
    /// Named break: cascade whenever there is a key window -- the link's
    /// window opens a step down and right of the one it covers.
    #[test]
    fn a_links_window_takes_the_clicked_frame() {
        let (clicked, key) = ((120, 80, 1013, 655), (40, 40, 960, 600));
        assert_eq!(placed(Some(clicked), Some(key)), Placed::Over(clicked));
        assert_eq!(placed(Some(clicked), None), Placed::Over(clicked));
        assert_eq!(placed(None, Some(key)), Placed::Cascade(key), "Command-N's");
        assert_eq!(placed::<()>(None, None), Placed::Default, "the launch's");
    }

    /// **A closing link's window gives the keyboard back to the window it
    /// covered, if that one is still open**: open, it is the one; closed
    /// first, none, and no other window is taken in its place; a window
    /// opened over nothing gives nothing back. Named breaks: give it back to
    /// none -- the clicked window is left behind; to the window opened last
    /// -- a closed-first window's place goes to another.
    #[test]
    fn a_links_window_gives_the_keyboard_back_to_the_one_it_covered() {
        let mut open = Windows::new();
        let clicked = open.open("clicked");
        let other = open.open("other");
        let link = open.open("link's");
        assert_eq!(focus_back(&open, Some(clicked)), Some(&"clicked"));
        assert_eq!(focus_back(&open, None), None, "opened over nothing");
        let _ = open.close(link);
        let _ = open.close(clicked);
        assert_eq!(focus_back(&open, Some(clicked)), None, "closed first");
        assert!(open.get(other).is_some(), "the control: another is open");
    }
}
