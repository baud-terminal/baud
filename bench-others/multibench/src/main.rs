// Same payloads, same real PTY, four terminal cores.
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::Processor;
use std::time::Instant;

/// ghostty's terminal core via libghostty-vt's C ABI. Three functions is the
/// whole surface we need: create, feed, destroy.
///
/// This replaces bench-others/ghostty-bench, a separate Zig harness that stalled
/// on ghostty's build-time-generated unicode_tables. libghostty-vt is now a
/// shipped artifact built by ghostty's own build.zig, which runs that codegen
/// itself — so ghostty measures here, in the same harness, through the same PTY,
/// against the same payloads as every other core. A separate harness was the
/// confound MEASUREMENTS.md warns about.
#[cfg(have_ghostty)]
mod ghostty {
    use std::ffi::c_void;
    pub type Terminal = *mut c_void;
    unsafe extern "C" {
        /// `allocator` may be NULL for ghostty's default. Returns 0 on success.
        pub fn ghostty_terminal_new(
            allocator: *const c_void,
            out: *mut Terminal,
            cols: u16,
            rows: u16,
        ) -> i32;
        pub fn ghostty_terminal_free(t: Terminal);
        /// Documented as infallible: malformed input is logged, never fatal.
        pub fn ghostty_terminal_vt_write(t: Terminal, data: *const u8, len: usize);
    }
}

/// baud's SIMD entry point only exists on x86-64, so naming it elsewhere does
/// not compile. Same shape as baud-bench's dispatch.
#[cfg(target_arch = "x86_64")]
unsafe fn baud_feed(t: &mut baud_core::Term, base: baud_core::BytePos, n: baud_core::ByteLen) {
    unsafe { t.feed_avx2(base, n) }
}
#[cfg(not(target_arch = "x86_64"))]
unsafe fn baud_feed(t: &mut baud_core::Term, base: baud_core::BytePos, n: baud_core::ByteLen) {
    t.feed(base, n)
}

const COLS: usize = 200;
const ROWS: usize = 50;

#[derive(Clone)]
struct Noop;
impl EventListener for Noop {
    fn send_event(&self, _: Event) {}
}
#[derive(Clone, Copy)]
struct Dim;
impl alacritty_terminal::grid::Dimensions for Dim {
    fn total_lines(&self) -> usize {
        ROWS
    }
    fn screen_lines(&self) -> usize {
        ROWS
    }
    fn columns(&self) -> usize {
        COLS
    }
}

/// Fork a writer on a PTY slave, hand the master fd to `sink` 4096 bytes at a time.
fn through_pty(payload: &[u8], total: usize, mut sink: impl FnMut(&[u8])) -> f64 {
    unsafe {
        let (mut m, mut s) = (0 as libc::c_int, 0 as libc::c_int);
        libc::openpty(
            &mut m,
            &mut s,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(s, &mut t);
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(s, libc::TCSANOW, &t);
        let pid = libc::fork();
        if pid == 0 {
            libc::close(m);
            let mut sent = 0usize;
            while sent < total {
                let mut o = 0usize;
                while o < payload.len() {
                    let w = libc::write(s, payload.as_ptr().add(o) as *const _, payload.len() - o);
                    if w <= 0 {
                        libc::close(s);
                        libc::_exit(0);
                    }
                    o += w as usize;
                }
                sent += payload.len();
            }
            libc::close(s);
            libc::_exit(0);
        }
        libc::close(s);
        let mut buf = vec![0u8; 4096];
        let mut got = 0usize;
        let t0 = Instant::now();
        loop {
            let r = libc::read(m, buf.as_mut_ptr() as *mut _, 4096);
            if r <= 0 {
                break;
            }
            sink(&buf[..r as usize]);
            got += r as usize;
            if got >= total {
                break;
            }
        }
        let v = got as f64 / t0.elapsed().as_secs_f64() / 1_048_576.0;
        libc::kill(pid, 9);
        let mut st = 0;
        libc::waitpid(pid, &mut st, 0);
        libc::close(m);
        v
    }
}

/// Median and range over `n` runs.
///
/// Was `best()`, which returned the maximum and discarded everything else. That
/// is an upward-biased estimator and, worse, it hides variance: two runs of
/// identical code here differed by 9.3%, which is larger than most differences
/// this table gets used to argue about. baud-bench has always reported
/// median-and-range; this makes the two harnesses agree on the statistic, so
/// only the payloads still differ between them.
fn stat(n: usize, mut f: impl FnMut() -> f64) -> (f64, f64, f64) {
    let mut v: Vec<f64> = (0..n).map(|_| f()).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (v[v.len() / 2], v[0], v[v.len() - 1])
}

/// One table row. Ranges are printed, not summarised away: if two cores' ranges
/// overlap there is no result, and the reader has to be able to see that.
fn row(label: &str, s: (f64, f64, f64), t: (f64, f64, f64)) {
    println!(
        "  {:<28} {:>7.1} [{:>5.1}-{:>5.1}]  {:>7.1} [{:>5.1}-{:>5.1}]",
        label, s.0, s.1, s.2, t.0, t.1, t.2
    );
}

fn main() {
    let total = 32usize << 20;

    let mut stream = Vec::new();
    const W: [&str; 8] = [
        "static", "inline", "struct", "return", "const", "size_t", "render", "parse",
    ];
    let mut col = 0;
    while stream.len() < (1 << 19) - 100 {
        if col > 70 {
            stream.extend_from_slice(b"\r\n");
            col = 0;
            continue;
        }
        if stream.len() % 37 == 0 {
            stream.extend_from_slice(b"\x1b[32m");
            continue;
        }
        let w = W[stream.len() % 8].as_bytes();
        stream.extend_from_slice(w);
        col += w.len();
        stream.push(b' ');
        col += 1;
    }
    let mut tui = Vec::new();
    let mut s = 0u64;
    while tui.len() < (1 << 19) - 200 {
        tui.extend_from_slice(format!("\x1b[{};{}H", s % 50 + 1, 1 + (s * 37) % 120).as_bytes());
        for _ in 0..6 {
            tui.extend_from_slice(b"pub struct ")
        }
        tui.extend_from_slice(b"\x1b[K");
        s += 1;
    }

    // The kitty harness cannot be a Rust core (its parser is a CPython object),
    // so it runs in Python — and a second implementation of these generators
    // would be a second thing to keep in sync. Dump the exact bytes instead, so
    // "same payloads" is a fact rather than a claim.
    if let Ok(dir) = std::env::var("DUMP_PAYLOADS") {
        std::fs::write(format!("{dir}/stream.bin"), &stream).expect("write stream.bin");
        std::fs::write(format!("{dir}/tui.bin"), &tui).expect("write tui.bin");
        eprintln!(
            "wrote {}/stream.bin ({} B) and {}/tui.bin ({} B)",
            dir,
            stream.len(),
            dir,
            tui.len()
        );
        return;
    }

    println!("  same payloads, same real PTY, 4096-byte reads, median of 9\n");
    println!(
        "  {:<28} {:>21}  {:>21}",
        "core", "streaming median [min-max]", "TUI median [min-max]"
    );

    // --- harness ceiling ---
    //
    // Not a terminal: the same PTY loop with a sink that discards. No core can
    // exceed it, and what a core costs is its distance from it. This exists
    // because kitty must be measured in a Python harness (its parser is a
    // CPython object), and comparing a number from that harness with one from
    // this one is only meaningful if both report what their own loop costs.
    let f = |p: &Vec<u8>| {
        stat(9, || {
            through_pty(p, total, |c| {
                std::hint::black_box(c);
            })
        })
    };
    row("ceiling (discard, no parse)", f(&stream), f(&tui));

    // --- baud ---
    let f = |p: &Vec<u8>| {
        stat(9, || {
            let mut st = baud_core::Storage::boxed();
            let mut t = baud_core::Term::new(&mut st, baud_core::OpMode::Runs);
            through_pty(p, total, |chunk| {
                let mut slot = t.read_slot();
                let base = slot.base();
                let fl = slot.write(baud_core::StreamByte::slice_from(chunk));
                unsafe { baud_feed(&mut t, base, fl.len()) };
                t.commit_read(fl.len());
            })
        })
    };
    row("baud (spans)", f(&stream), f(&tui));

    // --- alacritty_terminal ---
    let f = |p: &Vec<u8>| {
        stat(9, || {
            let mut cfg = Config::default();
            cfg.scrolling_history = 10_000;
            let mut term = ATerm::new(cfg, &Dim, Noop);
            let mut proc = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
            through_pty(p, total, |chunk| proc.advance(&mut term, chunk))
        })
    };
    row("alacritty_terminal", f(&stream), f(&tui));

    // --- ghostty, via libghostty-vt ---
    //
    // Skipped rather than faked when the static lib is absent: build.rs emits a
    // warning and the row does not print, which is visible. A zero would not be.
    #[cfg(have_ghostty)]
    {
        let f = |p: &Vec<u8>| {
            stat(9, || {
                let mut term: ghostty::Terminal = std::ptr::null_mut();
                let rc = unsafe {
                    ghostty::ghostty_terminal_new(
                        std::ptr::null(),
                        &mut term,
                        COLS as u16,
                        ROWS as u16,
                    )
                };
                assert_eq!(rc, 0, "ghostty_terminal_new failed with {rc}");
                let v = through_pty(p, total, |chunk| unsafe {
                    ghostty::ghostty_terminal_vt_write(term, chunk.as_ptr(), chunk.len());
                });
                unsafe { ghostty::ghostty_terminal_free(term) };
                v
            })
        };
        row("ghostty (libghostty-vt)", f(&stream), f(&tui));
    }
    #[cfg(not(have_ghostty))]
    println!(
        "  {:<28} {:>21}  {:>21}",
        "ghostty (libghostty-vt)", "n/a (lib not built)", "n/a"
    );

    // --- vt100 crate ---
    let f = |p: &Vec<u8>| {
        stat(9, || {
            let mut parser = vt100::Parser::new(ROWS as u16, COLS as u16, 10_000);
            through_pty(p, total, |chunk| parser.process(chunk))
        })
    };
    row("vt100 crate", f(&stream), f(&tui));

    // --- wezterm, full terminal model ---
    #[derive(Debug)]
    struct WzCfg;
    impl tattoy_wezterm_term::TerminalConfiguration for WzCfg {
        fn color_palette(&self) -> tattoy_wezterm_term::color::ColorPalette {
            tattoy_wezterm_term::color::ColorPalette::default()
        }
    }
    struct Sink;
    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let f = |p: &Vec<u8>| {
        stat(9, || {
            let size = tattoy_wezterm_term::TerminalSize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
                dpi: 0,
            };
            let mut term = tattoy_wezterm_term::Terminal::new(
                size,
                std::sync::Arc::new(WzCfg),
                "baud-bench",
                "1",
                Box::new(Sink),
            );
            through_pty(p, total, |chunk| term.advance_bytes(chunk))
        })
    };
    row("wezterm (full terminal)", f(&stream), f(&tui));

    // --- termwiz (wezterm's core) ---
    let f = |p: &Vec<u8>| {
        stat(9, || {
            let mut parser = termwiz::escape::parser::Parser::new();
            through_pty(p, total, |chunk| {
                parser.parse(chunk, |a| {
                    std::hint::black_box(a);
                });
            })
        })
    };
    row("termwiz (wezterm) parse only", f(&stream), f(&tui));
}
