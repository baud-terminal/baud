//! The tools `scripts/linux-*.sh` run over a headless compositor's output.
//!
//! - [`damage`] sums up a `linux-damage.sh` run;
//! - [`frames`] reads how the frames of `linux-frames.sh` reached the screen;
//! - [`pixels`] compares frames pixel for pixel, and reads a screenshot's
//!   size and sway's tree for `linux-pixels.sh`;
//! - [`trace`] keeps the pointer traffic of a compositor's log;
//! - [`pointer`] is the virtual pointer and keyboard `linux-pointer.sh`
//!   drives the app with.
//!
//! The examples of the same names hand these their arguments. Each is built
//! where it runs: [`damage`] and the Mac half of `linux-pixels.sh` by the
//! host's cargo, the rest in the container, as the app is. Their outputs are
//! the bytes the drivers and `tests/linux_tools/goldens.rs` hold, so
//! `\d`, `str.strip` and the rest read as they do in `key_latency::py`.
//! `MEASUREMENTS.md`, "The Linux tools are examples", has what was checked
//! against what.

pub mod damage;
pub mod frames;
pub mod pixels;
pub mod pointer;
pub mod text;
pub mod trace;

use std::io::Write;

/// How a tool ended: what it has for stdout, why it failed if it did, and
/// its exit code. Stdout goes out at the end in one write, so a reader that
/// stops early (`| head -1`) has been given everything it read.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Done {
    /// The whole of stdout.
    pub stdout: Vec<u8>,
    /// What stderr is told first, when the tool failed.
    pub why: String,
    /// The exit code.
    pub code: i32,
}

/// The code a program ends with when its stdout could not take what it
/// wrote. A closed pipe is not a panic: `head -1` may have gone.
pub const UNFLUSHED: i32 = 120;

impl Done {
    /// A tool that read everything: its stdout and its code.
    pub fn with(stdout: impl Into<Vec<u8>>, code: i32) -> Done {
        Done {
            stdout: stdout.into(),
            why: String::new(),
            code,
        }
    }

    /// A tool stopped by `why`, with what it had printed before it.
    pub fn failed(stdout: impl Into<Vec<u8>>, why: impl Into<String>) -> Done {
        Done {
            stdout: stdout.into(),
            why: why.into(),
            code: 1,
        }
    }

    /// Hands stdout and the reason to the process's streams, and returns the
    /// code to exit with.
    pub fn finish(&self) -> i32 {
        self.finish_to(&mut std::io::stdout().lock(), &mut std::io::stderr().lock())
    }

    /// [`Self::finish`] over any two writers. Nothing here panics on a
    /// writer that refuses: the code says so instead.
    pub fn finish_to(&self, out: &mut impl Write, err: &mut impl Write) -> i32 {
        if !self.why.is_empty() {
            // Telling stderr is best effort: there is nowhere else to say it.
            let _ = writeln!(err, "{}", self.why);
        }
        match out.write_all(&self.stdout).and_then(|()| out.flush()) {
            Ok(()) => self.code,
            Err(_) => UNFLUSHED,
        }
    }
}
