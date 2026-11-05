//! The terminal emulator as a process: read the pty, feed `baud-core`, render
//! with `baud-render`, present through the platform crate, encode input back.
//!
//! macOS first, through `macos-liquid-glass`: the window is the crate's,
//! the content view is its `DrawableView`, and this binary is the
//! `Responder` behind it -- one object on the main thread that owns the
//! terminal, the renderer, the presenter and the pty's write end. The pty's
//! read end lives on its own thread and hands chunks over a channel; the
//! display link's tick drains the channel, feeds the core, paints the rows
//! that changed and presents. That is "render on input, paced by the
//! display link" (D13): a chunk never waits longer than one frame.
//!
//! Linux second, through `baud-present-wayland`: one loop that waits on the
//! compositor's socket and the pty together, reads the pty where it is
//! polled, and presents on the CPU into `wl_shm` at the scale the
//! compositor prefers -- `linux.rs`, which is type-checked against the
//! Linux target from this Mac and has run only on headless compositors:
//! under weston its frames equal the CPU renderer's pixel for pixel, and
//! under sway a virtual keyboard types into it, a virtual pointer selects,
//! scrolls and clicks in it, a click at the grid's corners lands on its
//! cell at scales 1, 1.5 and 2, and its clipboard trades text with the
//! compositor's (`scripts/linux-pixels.sh`, `scripts/linux-pointer.sh`).
//! No screen has shown it.
//!
//! `unsafe` lives in `sys.rs`'s declarations; in `pty.rs`, `reload.rs`,
//! `fg.rs`, `hostlog.rs` and `discover.rs`; in `kitty_files.rs`'s
//! shared-memory read, in `pages.rs`'s release of freed image pages, in
//! `host.rs`'s release of the fonts a font reload replaced, in `lent.rs`'s
//! loans to a window's terminal and renderer, given back when it closes,
//! and in `heap.rs`, the macOS allocator, which forbid nothing and say why
//! at every call; `app.rs`, `link.rs` and `mark.rs` forbid it.

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
// Fonts by family name: the installed faces, read once at launch, and the
// face a family and a style pick among them, for both drivers.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod discover;
// fontconfig's configuration, the scan's directory list on Linux.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod fontconf;
// The system's face for a character no face of the chain draws: CoreText's
// answer on macOS, fontconfig's charset match over the scan on Linux.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod fallback;
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
// iTerm2's fireworks as both drivers time them: a show a request starts,
// held and ended on the clock, pure.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod fireworks;
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
// The macOS app's allocator: a block of 64 MiB or more is a mapping of
// its own, given back when freed.
#[cfg(target_os = "macos")]
mod heap;
#[cfg(target_os = "macos")]
mod latency;
// What a macOS window lends its terminal and renderer as `'static`, given
// back when it closes.
#[cfg(target_os = "macos")]
mod lent;
// Whether the system has a Hebrew keyboard layout, which DECHEBM needs:
// the classifier runs everywhere, the system's list is read on macOS.
#[cfg(any(target_os = "macos", test))]
mod layouts;
// What the activity lights' OH and CD read of the job in front.
#[cfg(target_os = "macos")]
mod front;
// The activity lights in the title strip: the Linux window has none.
#[cfg(target_os = "macos")]
mod lights;
// baud's mark in the title strip, beside the traffic lights.
#[cfg(target_os = "macos")]
mod mark;
// The title strip's accessories and the grid's top, windowed and in full
// screen.
#[cfg(target_os = "macos")]
mod strip;
// The state lights as words on DEC's indicator line, both drivers.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod line_lamps;
// The hover text over those words, drawn by baud on both drivers.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod line_tip;
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
// The activity lights on their own line, as the Linux driver feeds them.
#[cfg(any(target_os = "linux", test))]
mod own_line;
// Where the Linux driver sends a key's release: its tests run on every
// platform.
#[cfg(any(target_os = "linux", test))]
mod pressed;
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
// Real programs in a pty under the seat, for the tests only.
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod programs;
// baud filmed without a window, for scripts/readme-film.sh: the tests only.
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod film;
// The README's logo filmed without a window, for
// scripts/make-readme-logo.py: the tests only.
#[cfg(test)]
mod logo_film;
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
// The config read again when its files change: the kernel's watch on both
// platforms, the settle, and the reading that decides a change.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod reload;
// The five-second report both drivers log: its clock and its lines are
// pure, so its tests run on every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod report;
// Shell integration: the prompt marks baud gives the shells it starts.
mod shell;
// What a config read again changes in the running window, and the notice
// it shows: pure, so its tests run on every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod live;
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
// The launch splash's timing: pure, so its tests run on every platform,
// and both drivers time it through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod splash;
// The Linux driver's timed effects, off its one wait's timeout and the
// compositor's frame events: pure, so its tests run on every platform.
#[cfg(any(target_os = "linux", test))]
mod timed;
// The quad trail's driver is pure and its tests run on every platform;
// both drivers draw a trail through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod trail;
// A program's scroll read off the screen, which every trail takes away from
// a cursor carried with its line: pure, and both drivers take it through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod carried;
// What a program's title becomes on the window: pure, so its tests run on
// every platform, and both drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod title;
// A Wayland surface's points and the frame's pixels: pure, so its tests run
// on every platform, and the window operations size a Wayland window
// through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod surface;
// What a program's page-width request does to the window: pure, so its tests
// run on every platform, and both drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod width;
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod windows;
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
    // The installed fonts are read on a thread of their own from the
    // first instant, so the config's family names find them read: the
    // scan overlaps the window's and the GPU's setup instead of following
    // it, and the config waits only for what is left of it.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    std::thread::spawn(discover::Catalogue::system);
    log::init();
    #[cfg(target_os = "macos")]
    app::run();
    #[cfg(target_os = "linux")]
    linux::run();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    crate::warn!("baud-app presents on macOS and on Linux over Wayland, so far");
}
