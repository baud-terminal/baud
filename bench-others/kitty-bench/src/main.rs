use kitty_bench::Error;
use kitty_bench::args::{Payloads, payload_dir};
use kitty_bench::locate::locate;
use kitty_bench::pty::{CHUNK, Transfer, through_pty};
use kitty_bench::python::Kitty;
use kitty_bench::report::{Stat, header, row, stat};
use std::io::Write;
use std::path::{Path, PathBuf};

const COLS: i32 = 200;
const ROWS: i32 = 50;
const SCROLLBACK: i32 = 10_000;
const TOTAL: u64 = 32 << 20;
const REPS: usize = 9;

/// `REPS` runs of `one`, the median and range of their rates, and a line on
/// stderr saying how many bytes each run read.
fn reps(what: &str, mut one: impl FnMut() -> Result<Transfer, Error>) -> Result<Stat, Error> {
    let mut rates = Vec::with_capacity(REPS);
    let mut bytes = Vec::with_capacity(REPS);
    for _ in 0..REPS {
        let t = one()?;
        rates.push(t.mib_per_s);
        bytes.push(t.bytes);
    }
    eprintln!("  {what}: bytes read and parsed, each rep: {bytes:?}");
    Ok(stat(&mut rates))
}

fn print_line(line: &str) -> Result<(), Error> {
    writeln!(std::io::stdout(), "{line}").map_err(Error::Stdout)
}

fn run() -> Result<(), Error> {
    let dir = payload_dir(std::env::args_os())?;
    let payloads = Payloads::read(&dir)?;
    let location = locate(
        std::env::var_os("KITTY_ROOT").as_deref(),
        std::env::var_os("PATH").as_deref(),
        &[PathBuf::from("/Applications/kitty.app")],
    )?;

    print_line(&header(
        CHUNK,
        REPS,
        payloads.stream.len(),
        payloads.tui.len(),
    ))?;

    // The harness's other row, and nothing of kitty is loaded yet.
    let ceiling = |p: &[u8], what: &str| {
        reps(what, || {
            through_pty(p, TOTAL, |chunk: &[u8]| Ok(chunk.len()))
        })
    };
    let s = ceiling(&payloads.stream, "ceiling stream")?;
    let t = ceiling(&payloads.tui, "ceiling tui")?;
    print_line(&row("ceiling (discard, no parse)", s, t))?;

    let libpython = std::env::var_os("KITTY_LIBPYTHON").filter(|v| !v.is_empty());
    let kitty = Kitty::load(&location, libpython.as_deref().map(Path::new))?;
    eprintln!("  kitty: {}", location.extension.display());
    eprintln!("  python: {}", kitty.python_version());

    let parsed = |p: &[u8], what: &str| {
        reps(what, || {
            let mut screen = kitty.screen(ROWS, COLS, SCROLLBACK)?;
            through_pty(p, TOTAL, |chunk: &[u8]| screen.feed(chunk))
        })
    };
    let s = parsed(&payloads.stream, "kitty stream")?;
    let t = parsed(&payloads.tui, "kitty tui")?;
    print_line(&row("kitty (Screen.test_*)", s, t))?;
    eprintln!(
        "  callbacks the parser asked for: {}",
        kitty.callbacks_taken()
    );
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
