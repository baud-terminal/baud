use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::Processor;
use std::time::Instant;

#[derive(Clone)]
struct Noop;
impl EventListener for Noop {
    fn send_event(&self, _: Event) {}
}
#[derive(Clone, Copy)]
struct Dim {
    cols: usize,
    lines: usize,
}
impl alacritty_terminal::grid::Dimensions for Dim {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

const COLS: usize = 200;
const ROWS: usize = 50;

// byte-for-byte the same payload as ops2.c gen()
fn gen_tui(cap: usize, seq: &mut u64) -> Vec<u8> {
    const W: [&str; 10] = [
        "NORMAL",
        "main.rs",
        "fn render",
        "  let mut buf",
        "impl Term",
        "    match ev",
        "// TODO",
        "pub struct",
        "0x1b5b",
        "-- INSERT --",
    ];
    let mut b = Vec::with_capacity(cap);
    while b.len() < cap - 160 {
        let s = *seq;
        let r = s % ROWS as u64 + 1;
        let c = 1 + (s.wrapping_mul(37)) % (COLS as u64 - 80);
        b.extend_from_slice(format!("\x1b[{};{}H", r, c).as_bytes());
        if s % 211 == 0 {
            b.extend_from_slice(format!("\x1b[{}m", 30 + s % 8).as_bytes());
        }
        for k in 0..6 {
            b.extend_from_slice(W[((s + k) % 10) as usize].as_bytes());
            b.push(b' ');
        }
        b.extend_from_slice(b"\x1b[K");
        if s % 97 == 0 {
            b.extend_from_slice(b"\x1b[3X");
        }
        if s % 401 == 0 {
            b.extend_from_slice(b"\x1b[2L");
        }
        if s % 1601 == 0 {
            b.extend_from_slice(b"\x1b[J");
        }
        *seq += 1;
    }
    b
}

fn gen_stream(cap: usize, seq: &mut u64) -> Vec<u8> {
    const W: [&str; 14] = [
        "static", "inline", "struct", "return", "const", "size_t", "uint32_t", "buffer", "render",
        "terminal", "parse", "handle", "width", "index",
    ];
    let mut b = Vec::with_capacity(cap);
    let mut col = 0usize;
    while b.len() < cap - 96 {
        if col > 70 {
            b.push(b'\n');
            col = 0;
            continue;
        }
        if b.len() % 37 == 0 {
            b.extend_from_slice(format!("\x1b[{}m", 30 + (b.len() % 8)).as_bytes());
            continue;
        }
        let s = W[((*seq as usize) + b.len() / 13) % 14].as_bytes();
        b.extend_from_slice(s);
        col += s.len();
        b.push(b' ');
        col += 1;
    }
    *seq += 1;
    b
}

/// Same payload, but delivered through a real PTY at 4096 bytes a read — the
/// way alacritty itself gets its bytes (`alacritty_terminal/src/tty/unix.rs`
/// calls `openpty`). The in-memory path below skips syscalls and three kernel
/// copies, so it flatters alacritty relative to anything measured through a PTY.
fn run_pty(name: &str, payload: &[u8], total: usize) {
    let dim = Dim {
        cols: COLS,
        lines: ROWS,
    };
    let mut cfg = Config::default();
    cfg.scrolling_history = 10_000;
    let mut best = 0f64;
    for _ in 0..5 {
        unsafe {
            let (mut m, mut sfd) = (0 as libc::c_int, 0 as libc::c_int);
            libc::openpty(
                &mut m,
                &mut sfd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            let mut tio: libc::termios = std::mem::zeroed();
            libc::tcgetattr(sfd, &mut tio);
            libc::cfmakeraw(&mut tio);
            libc::tcsetattr(sfd, libc::TCSANOW, &tio);
            let pid = libc::fork();
            if pid == 0 {
                libc::close(m);
                let mut sent = 0usize;
                while sent < total {
                    let mut o = 0usize;
                    while o < payload.len() {
                        let w = libc::write(
                            sfd,
                            payload.as_ptr().add(o) as *const _,
                            payload.len() - o,
                        );
                        if w <= 0 {
                            libc::close(sfd);
                            libc::_exit(0);
                        }
                        o += w as usize;
                    }
                    sent += payload.len();
                }
                libc::close(sfd);
                libc::_exit(0);
            }
            libc::close(sfd);
            let mut term = ATerm::new(cfg.clone(), &dim, Noop);
            let mut p = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
            let mut buf = vec![0u8; 4096];
            let mut got = 0usize;
            let t0 = Instant::now();
            loop {
                let r = libc::read(m, buf.as_mut_ptr() as *mut _, 4096);
                if r <= 0 {
                    break;
                }
                let n = r as usize;
                p.advance(&mut term, &buf[..n]);
                got += n;
                if got >= total {
                    break;
                }
            }
            let v = got as f64 / t0.elapsed().as_secs_f64() / 1_048_576.0;
            libc::kill(pid, 9);
            let mut st = 0;
            libc::waitpid(pid, &mut st, 0);
            libc::close(m);
            if v > best {
                best = v;
            }
        }
    }
    println!("  {:<34} {:8.1} MB/s", name, best);
}

fn run(name: &str, payload: &[u8], total: usize) {
    let dim = Dim {
        cols: COLS,
        lines: ROWS,
    };
    let mut cfg = Config::default();
    cfg.scrolling_history = 10_000;
    let mut best = 0f64;
    for _ in 0..3 {
        let mut term = ATerm::new(cfg.clone(), &dim, Noop);
        let mut p = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
        for _ in 0..4 {
            p.advance(&mut term, payload);
        }
        let mut fed = 0usize;
        let t0 = Instant::now();
        while fed < total {
            p.advance(&mut term, payload);
            fed += payload.len();
        }
        let v = fed as f64 / t0.elapsed().as_secs_f64() / 1048576.0;
        if v > best {
            best = v;
        }
    }
    println!("  {:<28} {:8.1} MB/s", name, best);
}

fn main() {
    let mut seq = 0u64;
    let tui = gen_tui(1 << 19, &mut seq);
    let mut seq2 = 0u64;
    let stream = gen_stream(1 << 20, &mut seq2);
    println!("alacritty_terminal (vte + Term), no render\n");
    println!("  fed from memory, NO pty (skips syscalls + 3 kernel copies):");
    run("  TUI redraw payload", &tui, 48 << 20);
    run("  streaming text payload", &stream, 48 << 20);
    println!("\n  through a REAL PTY, 4096-byte reads (what alacritty actually does):");
    run_pty("  TUI redraw payload", &tui, 48 << 20);
    run_pty("  streaming text payload", &stream, 48 << 20);
}
