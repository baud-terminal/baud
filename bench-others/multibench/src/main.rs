// Same payloads, same real PTY, four terminal cores.
use std::time::Instant;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::Processor;

const COLS: usize = 200;
const ROWS: usize = 50;

#[derive(Clone)] struct Noop;
impl EventListener for Noop { fn send_event(&self, _: Event) {} }
#[derive(Clone, Copy)] struct Dim;
impl alacritty_terminal::grid::Dimensions for Dim {
    fn total_lines(&self) -> usize { ROWS }
    fn screen_lines(&self) -> usize { ROWS }
    fn columns(&self) -> usize { COLS }
}

/// Fork a writer on a PTY slave, hand the master fd to `sink` 4096 bytes at a time.
fn through_pty(payload: &[u8], total: usize, mut sink: impl FnMut(&[u8])) -> f64 {
    unsafe {
        let (mut m, mut s) = (0 as libc::c_int, 0 as libc::c_int);
        libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(s, &mut t); libc::cfmakeraw(&mut t); libc::tcsetattr(s, libc::TCSANOW, &t);
        let pid = libc::fork();
        if pid == 0 {
            libc::close(m);
            let mut sent = 0usize;
            while sent < total {
                let mut o = 0usize;
                while o < payload.len() {
                    let w = libc::write(s, payload.as_ptr().add(o) as *const _, payload.len() - o);
                    if w <= 0 { libc::close(s); libc::_exit(0); }
                    o += w as usize;
                }
                sent += payload.len();
            }
            libc::close(s); libc::_exit(0);
        }
        libc::close(s);
        let mut buf = vec![0u8; 4096];
        let mut got = 0usize;
        let t0 = Instant::now();
        loop {
            let r = libc::read(m, buf.as_mut_ptr() as *mut _, 4096);
            if r <= 0 { break; }
            sink(&buf[..r as usize]);
            got += r as usize;
            if got >= total { break; }
        }
        let v = got as f64 / t0.elapsed().as_secs_f64() / 1_048_576.0;
        libc::kill(pid, 9);
        let mut st = 0; libc::waitpid(pid, &mut st, 0);
        libc::close(m);
        v
    }
}

fn best(n: usize, mut f: impl FnMut() -> f64) -> f64 {
    let mut b = 0f64;
    for _ in 0..n { let v = f(); if v > b { b = v } }
    b
}

fn main() {
    let total = 32usize << 20;

    let mut stream = Vec::new();
    const W: [&str; 8] = ["static","inline","struct","return","const","size_t","render","parse"];
    let mut col = 0;
    while stream.len() < (1 << 19) - 100 {
        if col > 70 { stream.extend_from_slice(b"\r\n"); col = 0; continue }
        if stream.len() % 37 == 0 { stream.extend_from_slice(b"\x1b[32m"); continue }
        let w = W[stream.len() % 8].as_bytes();
        stream.extend_from_slice(w); col += w.len(); stream.push(b' '); col += 1;
    }
    let mut tui = Vec::new();
    let mut s = 0u64;
    while tui.len() < (1 << 19) - 200 {
        tui.extend_from_slice(format!("\x1b[{};{}H", s % 50 + 1, 1 + (s * 37) % 120).as_bytes());
        for _ in 0..6 { tui.extend_from_slice(b"pub struct ") }
        tui.extend_from_slice(b"\x1b[K");
        s += 1;
    }

    println!("  same payloads, same real PTY, 4096-byte reads, best of 5\n");
    println!("  {:<28} {:>12} {:>12}", "core", "streaming", "TUI redraw");

    // --- baud ---
    let f = |p: &Vec<u8>| best(5, || {
        let mut st = baud_core::Storage::boxed();
        let mut t = baud_core::Term::new(&mut st, baud_core::OpMode::Runs);
        through_pty(p, total, |chunk| {
            let mut slot = t.read_slot();
            let base = slot.base();
            let fl = slot.write(baud_core::StreamByte::slice_from(chunk));
            unsafe { t.feed_avx2(base, fl.len()) };
            t.commit_read(fl.len());
        })
    });
    println!("  {:<28} {:>9.1}    {:>9.1}", "baud (spans)", f(&stream), f(&tui));

    // --- alacritty_terminal ---
    let f = |p: &Vec<u8>| best(5, || {
        let mut cfg = Config::default(); cfg.scrolling_history = 10_000;
        let mut term = ATerm::new(cfg, &Dim, Noop);
        let mut proc = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
        through_pty(p, total, |chunk| proc.advance(&mut term, chunk))
    });
    println!("  {:<28} {:>9.1}    {:>9.1}", "alacritty_terminal", f(&stream), f(&tui));

    // --- vt100 crate ---
    let f = |p: &Vec<u8>| best(5, || {
        let mut parser = vt100::Parser::new(ROWS as u16, COLS as u16, 10_000);
        through_pty(p, total, |chunk| parser.process(chunk))
    });
    println!("  {:<28} {:>9.1}    {:>9.1}", "vt100 crate", f(&stream), f(&tui));

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
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { Ok(b.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let f = |p: &Vec<u8>| best(5, || {
        let size = tattoy_wezterm_term::TerminalSize {
            rows: ROWS, cols: COLS, pixel_width: 0, pixel_height: 0, dpi: 0,
        };
        let mut term = tattoy_wezterm_term::Terminal::new(
            size, std::sync::Arc::new(WzCfg), "baud-bench", "1", Box::new(Sink));
        through_pty(p, total, |chunk| term.advance_bytes(chunk))
    });
    println!("  {:<28} {:>9.1}    {:>9.1}", "wezterm (full terminal)", f(&stream), f(&tui));

    // --- termwiz (wezterm's core) ---
    let f = |p: &Vec<u8>| best(5, || {
        let mut parser = termwiz::escape::parser::Parser::new();
        through_pty(p, total, |chunk| {
            parser.parse(chunk, |a| { std::hint::black_box(a); });
        })
    });
    println!("  {:<28} {:>9.1}    {:>9.1}", "termwiz (wezterm) parse only", f(&stream), f(&tui));
}
