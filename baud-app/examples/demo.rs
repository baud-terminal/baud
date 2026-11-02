//! **The README's session, played in baud's window**: a real interactive
//! zsh and real programs at real speed, every command typed into the
//! window key by key -- btop, `cat README.md` and the wheel back through
//! it, six languages and a line of emoji in `cat hello.txt`, helix, a photo
//! through kitten, neo, asciiquarium and `clear`. With `--record` the same
//! run films it and encodes the README's GIF, and writes beside the GIF
//! where it came from, so the GIF is shown to be the session as it ran.
//!
//! ```text
//! cargo run --release --example demo              # the session
//! cargo run --release --example demo -- --dry     # all but the window
//! cargo run --release --example demo -- --record assets/readme/hero.gif
//! cargo run --example demo -- --verify assets/readme/hero.gif
//! ```
//!
//! Either from the repository's root or from `baud-app/`. macOS only but
//! for `--verify`; on any other system the rest says so and exits
//! non-zero.
//!
//! **What it needs**: nix, which every program, the photo and the photo's
//! converter come from, at the nixpkgs revision `scripts/readme-film.sh`
//! pins; network once, for the four open faces it caches; and for the
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
//! a scratch home with a `.zshrc` whose prompt is `baud ❯`, btop's and
//! helix's configs, and the shell's working directory under it holding a
//! copy of the README, `hello.txt` and the photo; baud's config, at 12
//! points with the cursor still and the pinned faces beside it; and baud
//! is started with nothing of this environment but `HOME`, `ZDOTDIR`,
//! `SHELL`, `PATH`, `LANG`, `TMPDIR`, `NOSYSZSHRC` and its own
//! (`BAUD_GRID`, `BAUD_CONFIG`, `BAUD_MOTION`, `BAUD_SCROLLER`, `BAUD_LOG`,
//! `BAUD_PTYLOG`): a 110x33 grid, full motion, the overlay scrollbar and a
//! pty log. The directory is removed after a session that ran to its end,
//! and kept otherwise.
//!
//! **What it checks first**, and `--dry` stops after: every program, the
//! photo and every face is the pinned one; every character of the README,
//! `hello.txt`, the prompt and neo's katakana is drawn by baud's own faces
//! or a pinned one, by baud-render's own face table, so no character of
//! those asks the system for a face -- the system tier stands behind every
//! stack and no setting keeps it out; what btop, helix, kitten, neo and
//! asciiquarium print themselves is not checked; and no file it wrote and
//! no variable baud is given names this machine -- the user, the host, a
//! home or temporary path -- but the scratch directory every path variable
//! must name. Then it prints the plan, with its holds and how long it
//! takes at least.
//!
//! **While it plays**: each step waits for the shell's next prompt by
//! counting `OSC 133 ; A` marks in the pty log, and each command is
//! checked echoed before its Return; before every key-down, key-up and
//! wheel event, baud must own the frontmost window and be the frontmost
//! application, and a wheel's point must lie in baud's window, or it stops
//! having posted nothing more and exits non-zero. At the end the window is
//! closed, and the pty log must name nothing of this machine.
//!
//! **`--record OUT.gif`**: the README's GIF from one command, filmed as it
//! is made. It refuses a repository whose tree differs from its commit,
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
