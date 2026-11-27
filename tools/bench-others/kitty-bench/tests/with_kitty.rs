//! The cycle against a real kitty, found as the binary finds one.
//!
//! The cycle in one test, because the interpreter comes up once a process and
//! belongs to the thread that started it, and the binary in a process of its
//! own. A run with no kitty lists both as ignored, with the reason. Asked for
//! with `--include-ignored` and no kitty, they fail.

use kitty_bench::Error;
use kitty_bench::locate::locate;
use kitty_bench::pty::through_pty;
use kitty_bench::python::Kitty;
use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore = "needs a kitty: KITTY_ROOT=<kitty> cargo test -- --include-ignored"]
fn the_cycle_runs_against_a_real_kitty_and_a_broken_one_is_an_error() {
    let location = locate(
        std::env::var_os("KITTY_ROOT").as_deref(),
        std::env::var_os("PATH").as_deref(),
        &[PathBuf::from("/Applications/kitty.app")],
    )
    .expect("a kitty to measure");
    let kitty = Kitty::load(&location, None).expect("kitty's interpreter comes up");

    // What the parser did with the bytes, read back through kitty.
    let mut screen = kitty.screen(5, 20, 100).unwrap();
    let bytes = b"hello \x1b[32mworld\x1b[0m\r\nbaud";
    assert_eq!(screen.feed(bytes).unwrap(), bytes.len());
    assert_eq!(screen.line(0).unwrap(), "hello world");
    assert_eq!(screen.line(1).unwrap(), "baud");
    assert_eq!(screen.cursor().unwrap(), (4, 1));

    // The callbacks absorb what would go to a window: a title, a bell, a reply.
    let before = kitty.callbacks_taken();
    let bytes = b"\x1b]2;a title\x07\x07\x1b[c";
    assert_eq!(screen.feed(bytes).unwrap(), bytes.len());
    assert!(
        kitty.callbacks_taken() > before,
        "no callback was asked for"
    );

    // A chunk larger than one read still all arrives, and in order.
    let mut screen = kitty.screen(5, 20, 100).unwrap();
    let bytes: Vec<u8> = b"0123456789\r\n".repeat(1000);
    assert_eq!(screen.feed(&bytes).unwrap(), bytes.len());
    // The cursor ends on the blank line below the last one written.
    assert_eq!(screen.line(3).unwrap(), "0123456789");
    assert_eq!(screen.cursor().unwrap(), (0, 4));

    // A broken cycle: not a write buffer. Reported, and no count comes back.
    let not_a_buffer = kitty.integer(5).unwrap();
    let e = screen.commit_into(b"abc", &not_a_buffer).unwrap_err();
    assert!(
        matches!(&e, Error::Python { call } if call == "test_commit_write_buffer"),
        "{e:?}"
    );

    // Through the PTY: a real rep, its bytes read all parsed.
    let payload: Vec<u8> = b"static inline \x1b[32mstruct\x1b[0m return\r\n".repeat(8000);
    let total = 1 << 18;
    let mut screen = kitty.screen(50, 200, 1000).unwrap();
    let t = through_pty(&payload, total, |chunk: &[u8]| screen.feed(chunk)).unwrap();
    assert!(t.bytes >= total, "{t:?}");
    assert!(t.mib_per_s > 0.0, "{t:?}");

    // The same with a screen that fails: the failure comes out of the run.
    let e = through_pty(&payload, total, |chunk: &[u8]| {
        screen.commit_into(chunk, &not_a_buffer)
    })
    .unwrap_err();
    assert!(matches!(e, Error::Python { .. }), "{e:?}");
}

/// The rep counts on stderr: nine reps of each of two payloads for each of two
/// rows, every byte count at least the 32 MiB the row is over.
fn rep_counts(stderr: &str) -> Vec<Vec<u64>> {
    stderr
        .lines()
        .filter_map(|l| l.split_once("each rep: "))
        .map(|(_, list)| {
            list.trim_matches(['[', ']'])
                .split(", ")
                .map(|n| n.parse().expect("a byte count"))
                .collect()
        })
        .collect()
}

#[test]
#[ignore = "needs a kitty: KITTY_ROOT=<kitty> cargo test -- --include-ignored"]
fn the_binary_prints_both_rows_against_a_real_kitty() {
    let dir = std::env::temp_dir().join(format!("kitty-bench-whole-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let payload = b"static inline \x1b[32mstruct\x1b[0m return\r\n".repeat(13_000);
    std::fs::write(dir.join("stream.bin"), &payload).unwrap();
    std::fs::write(dir.join("tui.bin"), &payload).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_kitty-bench"))
        .arg(&dir)
        .output()
        .expect("the harness runs");
    let _ = std::fs::remove_dir_all(&dir);
    let (out, err) = (
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
    );
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert!(out.contains("ceiling (discard, no parse)"), "{out}");
    assert!(out.contains("kitty (Screen.test_*)"), "{out}");

    let counts = rep_counts(&err);
    assert_eq!(counts.len(), 4, "{err}");
    for reps in counts {
        assert_eq!(reps.len(), 9, "{err}");
        assert!(reps.iter().all(|b| *b >= 32 << 20), "{err}");
    }

    // Only a parser asks for callbacks: a kitty row that only discarded has none.
    let asked = err
        .lines()
        .find_map(|l| l.trim().strip_prefix("callbacks the parser asked for: "))
        .expect("the callback count");
    assert!(asked.parse::<u64>().unwrap() > 0, "{err}");
}
