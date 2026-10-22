//! The terminal emulator as a process: read the pty, feed `baud-core`, render
//! with `baud-render`, present through the platform crate, encode input back.
//!
//! macOS first, through the owner's `macos-liquid-glass`: the window is the
//! crate's, the content view is its `DrawableView`, and this binary is the
//! `Responder` behind it -- one object on the main thread that owns the
//! terminal, the renderer, the presenter and the pty's write end. The pty's
//! read end lives on its own thread and hands chunks over a channel; the
//! display link's tick drains the channel, feeds the core, paints the rows
//! that changed and presents. That is "render on input, paced by the
//! display link" (D13): a chunk never waits longer than one frame.
//!
//! Linux second, through `baud-present-wayland`: one loop that waits on the
//! compositor's socket and the pty together, reads the pty where it is
//! polled, and presents on the CPU into `wl_shm` -- `linux.rs`, which is
//! type-checked against the Linux target from this Mac and has run only on
//! weston's headless backend, which displays nothing and was sent no input.
//!
//! `unsafe` lives in `pty.rs`, in `kitty_files.rs`'s shared-memory read
//! and in `pages.rs`'s release of freed image pages, which forbid nothing
//! and say why at every call; `app.rs` and `linux.rs` forbid it.

// What the system's accessibility settings ask -- reduced motion, increased
// contrast: both drivers read their own system and act through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod access;
#[cfg(target_os = "macos")]
mod app;
// Which half of the palette the Linux window draws under `theme = "auto"`:
// pure, so its tests run on every platform.
#[cfg(any(target_os = "linux", test))]
mod appearance;
// The scrollbar's press rules: pure, so its tests run on every platform,
// and both drivers take a pointer event through them.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod bar;
// The session bus, for the desktop's appearance and the file manager's
// reveal: the protocol over bytes is pure, so its tests run on every
// platform; only the Linux driver connects.
#[cfg(any(target_os = "linux", test))]
mod bus;
// The visual bell's state is pure and its tests run on every platform; both
// drivers read it, and so does the pty, for the one spelling of `TERM`.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod bell;
// The cursor's blink clock: pure, so its tests run on every platform; both
// drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod caret;
// What a program's `OSC 52` copy does to the pasteboard or to a Wayland
// seat's two selections: pure, so its tests run on every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod clip;
mod config;
// The font compiled into the binary: the default stack's first face.
mod embedded;
// A pointer event to the child or the selection, the cell under it, its
// clicks and a clean click's keys: pure, so its tests run on every platform,
// and both drivers route their pointer through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod gesture;
// When the pointer hides as the user types (XTSMPOINTER): pure, so its tests
// run on every platform, and both drivers act on it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod hide;
// The timed effects both drivers draw -- text blink's phases, the cursor a
// trail follows, the window the thumb shows: pure, so its tests run on
// every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod effects;
// Find in the scrollback: the keys, the field and the selection it makes.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod find;
// The foreground program and its folder, read from the kernel for the
// window's title (`title.rs`).
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod fg;
// What a program's `OSC 50` does to the font: the faces a name reaches and
// the size a step lands on, for both drivers.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod fonts;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod host;
// A program's log, written off the read loop (`?46`, `OSC 46`).
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod hostlog;
// The kitty file mediums' reader, which both drivers lend the core.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod kitty_files;
// What the app keeps over the screen across a read -- the selection's
// numbering and what a switch or a reset drops: pure, so its tests run on
// every platform, and both drivers keep a selection through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod keep;
#[cfg(target_os = "macos")]
mod latency;
// An input method's composition and the cell its candidate window opens
// at: pure, so its tests run on every platform, and both drivers draw
// through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod marked;
// The link policy is pure and its tests run on every platform; both
// drivers open a link through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod link;
#[cfg(target_os = "linux")]
mod linux;
mod log;
// What both drivers decide about a program's notification before the
// platform shows it: its tests run on every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod notify;
// The Linux driver's `xdg-open` for a link: its tests run on every
// platform.
#[cfg(any(target_os = "linux", test))]
mod opener;
// Freed image pages back to the system, which both drivers lend the core.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod pages;
// What a paste sends: both drivers paste through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod paste;
// A program's print jobs on their way to the print panel, and whether the
// core is told it has a printer: pure, so its tests run on every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod print;
// The theme files `[palette]` names: read at launch by both drivers, and
// its tests run on every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod palette;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod pty;
// ReGIS's built-in alphabet, drawn with the embedded face: both drivers
// lend it the core, and its tests run on every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod regis_font;
// The child's reads into the core and its answers out: pure, so its tests
// run on every platform, and both drivers feed through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod reads;
// xterm's readline modes, a click's cursor keys and a paste's shape: pure,
// so its tests run on every platform; both drivers have a pointer and a
// paste.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod readline;
// What a program's pointer shape (`OSC 22`) shows as among AppKit's
// cursors: pure, so its tests run on every platform; the Linux driver's
// shapes are `baud-present-wayland`'s.
#[cfg(any(target_os = "macos", test))]
mod pointer;
// The scrollbar's reveal is pure and its tests run on every platform; both
// drivers draw the thumb through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod reveal;
// The Linux driver's pointer between the compositor and the core: pure, so
// its tests run on every platform; only the Linux driver drives it.
#[cfg(any(target_os = "linux", test))]
mod seat;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod sys;
// The Linux driver's timed effects, off its one wait's timeout and the
// compositor's frame events: pure, so its tests run on every platform.
#[cfg(any(target_os = "linux", test))]
mod timed;
// The quad trail's driver is pure and its tests run on every platform;
// both drivers draw a trail through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod trail;
// What a program's title becomes on the window: pure, so its tests run on
// every platform, and both drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod title;
// What a program's page-width request does to the window: pure, so its tests
// run on every platform, and both drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod width;
// What a program's window operation does to the window: pure, so its tests
// run on every platform, and both drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod winops;
// The wheel's travel and its reports to a tracking application: pure, so its
// tests run on every platform, and both drivers scroll through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod wheel;

fn main() {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    host::startup_begin();
    log::init();
    #[cfg(target_os = "macos")]
    app::run();
    #[cfg(target_os = "linux")]
    linux::run();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    crate::warn!("baud-app presents on macOS and on Linux over Wayland, so far");
}
