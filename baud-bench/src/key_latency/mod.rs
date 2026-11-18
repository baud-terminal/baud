//! Measure keystroke-to-pixel latency for `baud`, leg by leg, as the app
//! times its own frames: the `key-latency` example.
//!
//! The keystroke instrument is not a change: without a number for typing, a
//! latency change cannot be kept or reverted. This drives it.
//!
//! # What it measures
//!
//! A helper (`scripts/key-post.swift`) posts plain letters as CGEvents
//! stamped with `CLOCK_UPTIME_RAW` -- the same mach host-time clock the app
//! reads for `NSEvent.timestamp`, the pty read, the GPU end, and the display
//! link. The app, run under `BAUD_KEYLAT=1`, prints one `keylat:` line per
//! key with an offset (microseconds) for each stage of the key's journey:
//!
//! ```text
//! keylat: seq=<n> event=<ns> handled=+<us> written=+<us> echo=+<us> \
//!         fed=+<us> commit=+<us> landed=+<us> shown=+<us> \
//!         vsync=+<us> vsync_hi=+<us>
//! ```
//!
//! `event` is absolute nanoseconds (so a posted key can be aligned to its
//! keylat line); every other field is an offset from `event`, or `-` for a
//! leg that did not happen (the CPU path has no `landed`; a landing whose
//! GPU end stamp is not on the clock is refused, `landed=-`, and the app
//! prints `keylat: landing refused ended=<ns> now=<ns>` beside it; an
//! expired record is emitted with the legs it has). The parser is STRICT
//! about the known tokens -- a record missing any of them is a parse
//! failure, counted and printed, never a silent `-` -- and tolerant of
//! extra tokens a newer app may add. The helper prints one `jitter
//! seed <pid> ...` line, then `post <seq> <ns> <char>` per key. Pairing the
//! posts to the records by order recovers, per key, the legs:
//!
//! ```text
//! stamp    = event    - post     (0 if the system kept our timestamp,
//!                                 else the delivery time it re-stamped)
//! deliver  = handled  - post     (post to the responder running)
//! write    = written  - handled  (Term::key + Pty::write)
//! child    = echo     - written  (the pty round trip: kernel + child)
//! wake     = fed      - echo     (reader thread wake to core fed)
//! build    = commit   - fed      (present: convert, encode, commit)
//! gpu      = landed   - commit   (GPU start to end; - on the CPU path)
//! show     = shown    - landed   (completion to surface on the layer)
//! vsync    = vsync    - shown    (surface set to the EARLY vsync bound)
//! vsync_hi = vsync_hi - shown    (surface set to the LATE vsync bound)
//! to_shown = shown    - post     (post to the surface on the layer;
//!                                 carries NO vsync quantisation)
//! total    = vsync    - post     (keystroke-to-pixel, early bound)
//! total_hi = vsync_hi - post     (keystroke-to-pixel, late bound)
//! ```
//!
//! `vsync` is the display-link target announced BEFORE the surface was set
//! -- the vsync the frame makes if it met the compositor's deadline -- and
//! `vsync_hi` is the target after it. `total` and `total_hi` are therefore
//! a BRACKET, not a measurement: on a fixed-period panel with no dropped
//! tick the pixel is composited somewhere in [vsync, vsync_hi], and
//! `scripts/key-pixel.swift`, run by hand beside a sweep, is the probe that
//! adjudicates where. Do not fold a probe run's totals into a sweep's: the
//! probe's own screen capture perturbs the app -- the sweep measured
//! post->shown at ~13 ms under capture against ~3 ms without, same child,
//! same box -- so a probe run measures the app under capture and its
//! `total`s are that run's alone. `to_shown` is the A/B number: it ends
//! where the app's own work ends and carries no display-period quantisation.
//!
//! # How to read it
//!
//! Per arm: the median (with n) and the interquartile range [q1-q3] of each
//! leg over the SETTLED rounds (round 1 is warm-up and discarded, as a cold
//! arm reads below its settled rate), the [min-max] beside it, plus keys
//! posted (the helper's count) / paired / completed / excluded, and how many
//! shared a frame. With `--aa` a second arm runs the SAME binary again; the
//! two are printed side by side and the noise floor is given PER LEG as the
//! larger of the two medians' difference and either arm's interquartile
//! range -- never the max-min spread, which one fat-tail outlier owns (a
//! single 163 ms echo once made it read 169 ms). A leg that moves by less
//! than its own floor is not a result. Take it on mains, on an idle box; the
//! header says which you had.
//!
//! With more than one non-`aa` arm the driver also prints, per leg,
//! `arm - base` beside that A/A floor, and marks a row `result` only when
//! |delta| exceeds the floor AND the two arms' interquartile ranges are
//! disjoint; every other row says `not a result` and why (within the floor,
//! or the IQRs overlap). Without `--aa` there is no floor and no row can be
//! a result. This table is what a latency change -- pacing the GPU by
//! completion for a keystroke, skipping the present when nothing changed --
//! is read by.
//!
//! A key is `completed` when it paired and reached `vsync`; `excluded` when
//! the app's `seq` is not the helper's, its `event` precedes its `post` or
//! trails it by >= the pairing window (100 ms, or the MINIMUM jittered gap -
//! 5 ms when that minimum is under 100 ms), any leg after `stamp` is
//! negative (a stamp off the clock), or its landing was refused (see below).
//! Legs with `-` simply do not feed their median, and each leg's n says how
//! many did.
//!
//! Two exclusions look like defects and are not. A row with `child < 0` is
//! the echo stamped INSIDE the write: the line discipline echoes the byte
//! during the `write(2)` that carries it, so the reader thread can stamp the
//! echo before the post-write stamp lands and `echo - written` comes out a
//! few microseconds negative. That is a non-duration; the negative-leg rule
//! excludes the row and the per-round `negative=` count says how many (2 of
//! 240 keys on the first sweep, -3.6 and -1.1 us). And a run in which the
//! app printed `keylat: landing refused` is NOT QUOTABLE: the GPU end stamp
//! was off the clock, and a refused present can stamp `shown` on a record
//! whose real frame has not presented yet, so a clean-looking low `to_shown`
//! in such a run is not clean. Every row of that run that reached the GPU
//! (`commit` set) and has no landing (`landed=-`) is excluded as
//! `refused-landing`; the arm's summary and the sweep's last lines say NOT
//! QUOTABLE and name the run. A run on the CPU path has `landed=-` on every
//! row and no refusal line, and is untouched.
//!
//! The arm order rotates every round, so the A/A arm is not always last and
//! a position artefact cannot pass as an arm. Each arm's path and sha256 are
//! printed; two byte-identical arms are warned about unless one is `aa`,
//! which is byte-identical by design.
//!
//! `--gap` is the MEAN inter-key gap. The helper sleeps a uniform random
//! duration in [gap - gap/4, gap + gap/4] before each key after the first (a
//! gap of 0 stays 0: typeahead), seeded from baud's pid, so a run is
//! reproducible and the pid is printed per round. A FIXED gap can phase-lock
//! to the refresh: `[[source: hume-latency-tester]]` drove a fixed inter-key
//! delay and read a false SD of 1 ms, because every key then lands at the
//! same point in the display period and the vsync term stops varying -- the
//! noise floor reads smaller than it is, on exactly the legs (`vsync`,
//! `total`) that carry the period. The `post` lines carry the real stamps,
//! so nothing in the analysis changes with the jitter; the one parameter that
//! follows it is the pairing window, which is set from the MINIMUM jittered
//! gap so a one-key shift still cannot fit inside it.
//!
//! `--child cat` measures the tty's own echo: `cat` never reads a byte before
//! Return, so the line discipline is the only thing that answers and there is
//! no program in the loop. `--child shell` runs the login shell, what a user
//! feels. `--child none` runs `stty -echo; sleep 60`, the no-echo control:
//! every key is posted and none completes, and the sweep exits 1 to say so.
//! (`sleep` alone is NOT a no-echo control -- the line discipline echoes
//! whatever the child does.) `--child stream` is `cat`'s echo under a child
//! that prints a line every 2 ms and never reads: a build log scrolling while
//! the user types. `--child mute` is a prompt that never answers: the OSC 133
//! marks a shell sends around its prompt, then echo off and sleep, so no key
//! typed at it echoes. `stream` is the workload
//! row 6 of note N is about -- with output arriving several times a display
//! period, the frame loop's "draw now on a wake only if a frame's time has
//! passed" gate is never open, so a key's echo is paced by the tick, and
//! `build` (fed to commit) carries the wait. With an isolated key the gate is
//! always open (row 0 read `build` at 91 us), which is why `--child cat`
//! cannot show row 6.
//!
//! # Exit codes
//!
//! - 0: every run had completed keys (or `--selftest` passed)
//! - 1: a run reported zero completed keys, or bad usage of a value
//! - 2: the Swift helper REFUSED (baud did not own the front window) -- the
//!   guard did its job; whatever it posted before refusing is paired -- or
//!   the arguments did not parse
//! - 3: `--selftest` failed
//! - 4: the helper failed (an exit other than 0 or 3): its code and output
//!   are printed and the sweep is aborted
//!
//! ```text
//! cargo run --release -p baud-bench --example key-latency -- [--rounds 5] \
//!     [--keys 30] [--gap 150] [--child cat|shell|none|stream|mute] \
//!     [--arm name=path] [--aa] [--grid 120x40]
//! cargo run --release -p baud-bench --example key-latency -- --selftest
//! ```
//!
//! Run it from the repository's root, where the helper's path and the default
//! arm's resolve. The self-test posts nothing and opens no window; a sweep
//! does both.

pub mod cli;
pub mod parse;
pub mod poster;
pub mod py;
pub mod report;
pub mod rows;
pub mod selftest;
pub mod sha256;
pub mod stats;
pub mod sweep;

use cli::Cli;

/// The driver: argv without the program's name, to the exit code.
pub fn run(argv: &[String]) -> i32 {
    if argv.first().is_some_and(|a| a == sweep::STREAM_FLAG) {
        return sweep::stream_child();
    }
    match cli::parse(argv) {
        Cli::Help => {
            print!("{}", cli::HELP);
            0
        }
        Cli::Usage(why) => {
            eprint!("{}", cli::HELP.split("\n\n").next().unwrap_or(""));
            eprintln!("\nkey-latency: error: {why}");
            2
        }
        Cli::Selftest => {
            // `BAUD_KEYLAT_FIXTURES` points the self-test at another pair.
            let dir = std::env::var("BAUD_KEYLAT_FIXTURES")
                .unwrap_or_else(|_| selftest::DEFAULT_FIXTURES.to_string());
            match selftest::selftest(&dir) {
                Ok(o) => {
                    print!("{}", o.stdout);
                    o.code
                }
                Err(why) => {
                    eprintln!("key-latency: {why}");
                    1
                }
            }
        }
        Cli::Sweep(mut args) => {
            if args.rounds < 1 || args.keys < 1 || args.gap.0 < 0 {
                println!("--rounds and --keys must be >= 1, --gap >= 0");
                return 1;
            }
            if args.arm.is_empty() {
                args.arm = vec!["base=target/release/baud".to_string()];
            }
            for a in &args.arm {
                if !a.contains('=') {
                    println!("--arm wants name=path, got {}", py::repr_str(a));
                    return 1;
                }
            }
            match sweep::sweep(&args) {
                Ok(code) => code,
                Err(why) => {
                    eprintln!("key-latency: {why}");
                    1
                }
            }
        }
    }
}
