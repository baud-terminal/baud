//! **The README's session, played in baud's window**: a real interactive
//! zsh and real programs at real speed, every command typed into the
//! window key by key -- btop, `cat README.md` and the wheel back through
//! it, six languages and a line of emoji in `cat hello.txt`, ssh into a
//! container on this Mac and fastfetch there, helix, a photo through
//! kitten, neo, pipes-rs and `clear`. With `--record` the same run films
//! it and encodes the README's GIF, and writes beside the GIF where it
//! came from, so the GIF is shown to be the session as it ran.
//!
//! ```text
//! cargo run --release --example demo              # the session
//! cargo run --release --example demo -- --dry     # all but the window
//! cargo run --release --example demo -- --headless DIR   # no window
//! cargo run --release --example demo -- --record assets/readme/hero.gif \
//!     --forbidden LIST --controls FILE
//! cargo run --example demo -- --verify assets/readme/hero.gif
//! ```
//!
//! Either from the repository's root or from `baud-app/`. macOS only but
//! for `--verify`; on any other system the rest says so and exits
//! non-zero.
//!
//! **What it needs**: nix, which every program, the photo and the photo's
//! converter come from, at the nixpkgs revision `scripts/readme-film.sh`
//! pins; network once, for the four open faces it caches; colima and
//! docker, for the container (`demo/container.rs`), whose image the
//! repository's flake makes with nixpkgs' `dockerTools` from packages
//! cache.nixos.org holds, so no Linux builder -- colima is started if it
//! is stopped and stopped again at the end; and for the
//! session itself, Accessibility for the terminal it runs in (System
//! Settings > Privacy & Security > Accessibility), which it checks before
//! posting anything and, missing, names and stops at. `--record` also needs
//! Screen Recording for that terminal (Privacy & Security > Screen & System
//! Audio Recording), checked the same way, and Xcode, whose `swiftc`
//! compiles the recorder.
//!
//! **Which baud**: the current one. It runs `cargo build -p baud` -- the
//! cargo that ran it, in its own profile and target directory, read off
//! where its binary is -- and launches the `baud` that build leaves
//! beside it. A profile given only as `--config` on the command line is
//! not seen by that build.
//!
//! **What it makes**, in a fresh directory in the system's temporary one:
//! a scratch home with a `.zshrc` whose prompt is `baud ❯` and whose `ssh`
//! reads the scratch home's ssh config, btop's and helix's configs, the
//! session's two ssh keys, and the shell's working directory under it
//! holding a copy of the README, `hello.txt` and the photo; the
//! container's image, streamed by the repository flake's `demo-container`
//! and pinned by its SHA-256; baud's config, at
//! 12 points with the cursor still, the window solid -- opaque under
//! either appearance, its font and its size not a program's to change --
//! and the pinned faces beside it; and baud
//! is started with nothing of this environment but `HOME`, `ZDOTDIR`,
//! `SHELL`, `PATH`, `LANG`, `TMPDIR`, `NOSYSZSHRC` and its own
//! (`BAUD_GRID`, `BAUD_SCREEN`, `BAUD_CONFIG`, `BAUD_MOTION`,
//! `BAUD_SCROLLER`, `BAUD_LOG`, `BAUD_PTYLOG`): a 110x33 grid, the window
//! centred on the primary display, full motion, the overlay scrollbar and
//! a pty log. The directory is removed after a session that ran to its
//! end, and kept otherwise.
//!
//! **What it checks first**, and `--dry` stops after: every program, the
//! photo and every face is the pinned one; every character of the README,
//! `hello.txt`, the prompt, neo's katakana and pipes-rs's pipes is drawn
//! by baud's own faces or a pinned one, by baud-render's own face table, so
//! no character of those asks the system for a face -- the system tier
//! stands behind every stack and no setting keeps it out; what btop,
//! helix, kitten and neo print themselves is not checked; and no file it
//! wrote and no variable baud is given names this machine -- the user, the
//! host, a home or temporary path -- but the scratch directory every path
//! variable must name. Then it prints the plan, with its holds and how
//! long it takes at least.
//!
//! **Before it plays**: baud's window must keep one frame for half a
//! second, the frame it was first seen at, or it stops: the film holds the
//! launch where the window was first seen. Filmed, the window must also
//! lie wholly on the display the recorder films. `BAUD_SCREEN=primary`
//! opens it there, centred on the primary display, which is CoreGraphics'
//! main display (`[[source: apple-nsscreen-screens]]`,
//! `[[source: apple-cgmaindisplayid]]`), rather than on the display with
//! the key window, where AppKit centres a new one: in one try that was a
//! second display, and in that try the window also moved 9 points by the
//! first prompt, for a reason not reproduced here.
//!
//! **While it plays**: each step waits for the shell's next prompt by
//! counting `OSC 133 ; A` marks in the pty log, or in the container for
//! its prompt's text, and each command is checked echoed before its
//! Return; before every key-down, key-up and wheel event, baud must own
//! the frontmost window and be the frontmost application, and a wheel's
//! point must lie in baud's window, or it stops having posted nothing more
//! and exits non-zero. At the end the window is closed, the container and
//! its image removed, and the pty log must name nothing of this machine;
//! with `--forbidden LIST --controls FILE` it must also hold nothing LIST
//! forbids (`scripts/readme-forbidden.py`, a list kept outside the
//! repository, each pattern found in its control first). Ctrl-C stops the
//! session at its next wait, and the container, its image and colima are
//! put back as they are at the end; it exits 130.
//!
//! **`--headless DIR`**: the session with no window, the container
//! running: baud's own test of it (`film::session` in `baud-app/src/`)
//! types every step into baud's login shell through baud's core, feeds
//! the nine activity lights as the macOS driver does, and fails unless
//! each but CS lights; the last frame of every hold, drawn by baud's CPU
//! renderer on the opaque page, goes into DIR as a PNG.
//! `BAUD_SESSION_POINTS` draws at another size and `BAUD_SESSION_WITHOUT`
//! takes a command out, for a perturbation.
//!
//! **`--record OUT.gif`**: the README's GIF from one command, filmed as it
//! is made, `--forbidden` and `--controls` required. It refuses a
//! repository whose tree differs from its commit,
//! unless `--allow-dirty`, which the provenance file then records. The
//! recorder, `scripts/trail-record.swift` compiled by Xcode's `swiftc`,
//! films the display, every application already running left out, and baud
//! is launched only once it films, so the GIF opens on the launch, the
//! moment before baud's window appears included: a window can be filmed
//! only once it exists, and in a take of this session baud's was on screen
//! before a capture of it could start. At the end the film is cropped to
//! baud's window, which must not have moved, sampled at 30 frames a second
//! by ffmpeg and encoded by gifski at `--quality 80 --lossy-quality 100
//! --motion-quality 100 --fps 30 --width 1164`, both from the pinned
//! nixpkgs: no cut, no pad, no change of speed. The recorder ends the film
//! at the stop and says where, the film must play to there and no further,
//! and the GIF must run as long as the film. Beside the GIF goes
//! `OUT.gif.provenance.toml`: the commit built and filmed and whether its
//! tree was clean, the command, every step with its time since the launch,
//! the nixpkgs revision and each tool's store path, the SHA-256 of each
//! pinned face, the photo, the raw film and the GIF, the recorder's and
//! encoder's arguments and the macOS version -- checked, before it is
//! written, to name nothing of this machine. The raw film stays in cargo's
//! target directory, at `demo/<name>.raw.mp4`. With `--dry`, it compiles
//! the recorder; builds the recorder's writer alone, with no capture and no
//! window, and checks that the films it writes, one ending on a still tail,
//! play to the stop and no further and make as many frames as ffmpeg's
//! fps=30 samples from the length it plays; runs the encoder, the
//! provenance file and `--verify` on two seconds of ffmpeg's `testsrc`; and
//! checks that `--verify` refuses the GIF with one byte changed.
//!
//! **`--verify GIF`**: reads the provenance file beside it and checks that
//! the GIF's SHA-256 and length are the recorded ones and that the commit
//! is in this repository; on any system with git and `shasum` or
//! `sha256sum`.
//!
//! **For a recorder outside**: `--pid-file PATH` gets baud's pid once it is
//! running, `--events PATH` a line per step, seconds since launch first,
//! and `--done PATH` is created when the session ends: 1.5 s before the
//! window closes after a session that ran to its end, and at once after
//! one that stopped.
//!
//! The steps are data: `baud-app/examples/demo/session.rs`.

#![deny(unsafe_code)]

// The modules are in `baud-app/examples/demo/`; a crate root looks for
// them beside itself.

#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/container.rs"]
mod container;
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/film.rs"]
mod film;
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/fonts.rs"]
mod fonts;
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/guard.rs"]
mod guard;
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/keys.rs"]
mod keys;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
#[path = "demo/mac.rs"]
mod mac;
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/names.rs"]
mod names;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/provenance.rs"]
mod provenance;
#[cfg(target_os = "macos")]
#[path = "demo/record.rs"]
mod record;
#[cfg(target_os = "macos")]
#[path = "demo/run.rs"]
mod run;
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/scratch.rs"]
mod scratch;
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "demo/session.rs"]
mod session;

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    run::main()
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, gif] = args.as_slice() {
        if flag == "--verify" {
            let gif = std::path::Path::new(gif);
            return match provenance::verify(gif, &provenance::repository()) {
                Ok(said) => {
                    for l in said {
                        println!("{l}");
                    }
                    println!(
                        "verified: {} is the GIF its provenance file describes",
                        gif.display()
                    );
                    std::process::ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("demo: {e}");
                    std::process::ExitCode::FAILURE
                }
            };
        }
    }
    eprintln!("demo: the README session runs on macOS, in baud's macOS window; --verify runs here");
    std::process::ExitCode::from(2)
}
