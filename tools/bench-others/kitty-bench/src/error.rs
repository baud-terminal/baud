use std::path::PathBuf;

/// Every way a run ends with no number. `main` prints one of these and exits 1,
/// so nothing here reaches the table.
#[derive(Debug)]
pub enum Error {
    /// No payload directory on the command line.
    Usage { argv0: String },
    /// `stream.bin` or `tui.bin` would not read.
    Payload {
        path: PathBuf,
        source: std::io::Error,
    },
    /// An empty payload. The writer would loop for ever and send nothing.
    EmptyPayload { path: PathBuf },
    /// `KITTY_ROOT` named a directory with no extension under it, or nothing
    /// named one and none was found.
    NoKitty { looked: Vec<PathBuf> },
    /// The loader refused a library.
    Load { library: PathBuf, reason: String },
    /// The library loaded and is not a Python: it lacks this symbol.
    NotPython { library: PathBuf, symbol: String },
    /// A call into the interpreter failed; its traceback went to stderr.
    Python { call: String },
    /// `commit` said it took no bytes or more than it was offered.
    BrokenCycle { offered: usize, committed: usize },
    /// The sink accounted for a different number of bytes than were read.
    WrongByteCount { read: u64, parsed: u64 },
    /// The PTY ended before `total` bytes arrived.
    ShortRead { got: u64, total: u64 },
    /// A PTY system call failed.
    Pty {
        call: &'static str,
        source: std::io::Error,
    },
    /// Writing the table failed.
    Stdout(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Usage { argv0 } => write!(
                f,
                "usage: {argv0} <payload-dir>   (see DUMP_PAYLOADS in tools/bench-others/README.md)"
            ),
            Error::Payload { path, source } => write!(f, "{}: {source}", path.display()),
            Error::EmptyPayload { path } => write!(f, "{}: empty payload", path.display()),
            Error::NoKitty { looked } => {
                write!(f, "no kitty/fast_data_types.so; set KITTY_ROOT. Looked at:")?;
                for p in looked {
                    write!(f, "\n  {}", p.display())?;
                }
                Ok(())
            }
            Error::Load { library, reason } => {
                write!(f, "cannot load {}: {reason}", library.display())
            }
            Error::NotPython { library, symbol } => write!(
                f,
                "{} brought no {symbol}: no Python came with it; set KITTY_LIBPYTHON to the \
                 libpython this kitty was built against",
                library.display()
            ),
            Error::Python { call } => write!(f, "{call} failed in kitty's interpreter"),
            Error::BrokenCycle { offered, committed } => write!(
                f,
                "broken cycle: offered {offered} bytes, test_commit_write_buffer reported {committed}"
            ),
            Error::WrongByteCount { read, parsed } => {
                write!(f, "wrong byte count: read {read} bytes, parsed {parsed}")
            }
            Error::ShortRead { got, total } => {
                write!(f, "the PTY ended at {got} bytes of {total}")
            }
            Error::Pty { call, source } => write!(f, "{call}: {source}"),
            Error::Stdout(source) => write!(f, "stdout: {source}"),
        }
    }
}

impl std::error::Error for Error {}
