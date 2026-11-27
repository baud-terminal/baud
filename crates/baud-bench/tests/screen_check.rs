//! The screen check of the pty comparison (`baud_bench::host::screen_check`,
//! `tools/scripts/vt-child.sh`, the `vt-screens` example), and its positive
//! controls: a wrong cell or a wrong cursor must read UNEQUAL.
//!
//! The controls run `tools/scripts/vt-child.sh` itself on a pty whose other end is
//! a headless baud-core, which answers the child's questions as a terminal
//! does. Nothing here starts a terminal application or opens a window. The
//! child's `vt-reply` is this test binary, run again as [`as_vt_reply`]
//! through a two-line wrapper, since a test cannot name an example's
//! executable.

use baud_bench::capture::pty::{Child, Environ, Lookup, Spawn};
use baud_bench::host::screen_check::{
    self, CHECKSUM_ID, Differs, MARK, Said, Verdict, judge, questions, read, reference, replies,
};
use baud_bench::host::{Live, vt_reply};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, ReadLen, WinSize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The size the controls run at, larger than [`MARK`] both ways.
const ROWS: u16 = 24;
const COLS: u16 = 80;

/// Text, a bare line feed (which the pty's output processing turns into a
/// carriage return and a line feed), a coloured background, a wide
/// character and two cursor moves: what the checksum and the cursor are
/// asked to agree on.
const RECORDING: &[u8] =
    "one\ntwo \x1b[41mred\x1b[m \u{4e2d}\u{6587}\r\n\x1b[5;10Hthree\x1b[2;3H".as_bytes();

#[test]
fn the_reply_is_read_for_the_cursor_the_replay_left_and_the_checksum() {
    // A reply to a query inside the recording first, then the cursor after
    // the replay, the checksum report, and the answer at the mark.
    let mut reply = b"\x1b[3;1R".to_vec();
    reply.extend_from_slice(b"\x1b[7;12R");
    reply.extend_from_slice(format!("\x1bP{CHECKSUM_ID}!~0A1F\x1b\\").as_bytes());
    reply.extend_from_slice(format!("\x1b[{};{}R", MARK.0, MARK.1).as_bytes());
    assert_eq!(
        read(&reply),
        Said {
            cursor: Some((7, 12)),
            checksum: Some(0x0A1F)
        }
    );
    // The eight-bit introducers read the same.
    let mut eight = vec![0x9b];
    eight.extend_from_slice(b"7;12R");
    eight.push(0x90);
    eight.extend_from_slice(format!("{CHECKSUM_ID}!~0A1F").as_bytes());
    eight.push(0x9c);
    eight.push(0x9b);
    eight.extend_from_slice(format!("{};{}R", MARK.0, MARK.1).as_bytes());
    assert_eq!(read(&eight), read(&reply));
    // A checksum report with another id is not this one, and an extended
    // cursor report is not a cursor report.
    let other = format!("\x1b[?7;12;1R\x1bP1!~0A1F\x1b\\\x1b[{};{}R", MARK.0, MARK.1);
    assert_eq!(
        read(other.as_bytes()),
        Said {
            cursor: None,
            checksum: None
        }
    );
}

#[test]
fn vt_reply_hex_reads_back_and_none_reads_as_nothing() {
    assert_eq!(
        screen_check::unhex("1b5b31333b333752"),
        Some(b"\x1b[13;37R".to_vec())
    );
    assert_eq!(screen_check::unhex("none"), None);
    assert_eq!(screen_check::unhex(""), None);
}

#[test]
fn the_tty_puts_a_carriage_return_before_every_line_feed() {
    assert_eq!(
        screen_check::through_the_tty(b"a\nb\r\nc"),
        b"a\r\nb\r\r\nc"
    );
    // Without it the bare line feed leaves the cursor at another column.
    // The checksum is a sum over the cells, so text moved along its row
    // leaves it as it was: the cursor is what tells the two apart here.
    let bare = b"one\ntwo";
    assert_eq!(reference(bare, ROWS, COLS).cursor, Some((2, 7)));
    assert_eq!(
        reference(&screen_check::through_the_tty(bare), ROWS, COLS).cursor,
        Some((2, 4))
    );
}

#[test]
fn the_reference_is_what_baud_core_answers() {
    let want = reference(RECORDING, ROWS, COLS);
    assert_eq!(want.cursor, Some((2, 3)), "the recording ends at 2;3");
    assert!(want.checksum.is_some(), "baud-core answers DECRQCRA");
    assert_eq!(judge("baud", want, want), Verdict::Same);
}

#[test]
fn a_planted_wrong_cell_changes_baud_core_s_checksum_and_not_its_cursor() {
    let want = reference(RECORDING, ROWS, COLS);
    let planted = read(&replies(RECORDING, b"\x1b7\x1b[1;1HX\x1b8", ROWS, COLS));
    assert_eq!(
        planted.cursor, want.cursor,
        "DECSC and DECRC keep the cursor"
    );
    assert_ne!(
        planted.checksum, want.checksum,
        "the checksum sees the cell"
    );
    assert!(matches!(
        judge("baud", planted, want),
        Verdict::Unequal(Differs::Checksum(..))
    ));
}

#[test]
fn another_terminal_is_judged_on_the_cursor_alone() {
    let want = reference(RECORDING, ROWS, COLS);
    let summed_otherwise = Said {
        cursor: want.cursor,
        checksum: Some(want.checksum.unwrap_or(0) ^ 1),
    };
    assert_eq!(
        judge("ghostty", summed_otherwise, want),
        Verdict::CursorOnly
    );
    assert_eq!(
        judge(
            "alacritty",
            Said {
                cursor: want.cursor,
                checksum: None
            },
            want
        ),
        Verdict::CursorOnly
    );
    let moved = Said {
        cursor: Some((9, 9)),
        checksum: None,
    };
    assert!(matches!(
        judge("alacritty", moved, want),
        Verdict::Unequal(Differs::Cursor((9, 9), (2, 3)))
    ));
    assert!(!judge("alacritty", moved, want).rated());
    assert!(Verdict::CursorOnly.says().contains("not compared"));
}

#[test]
fn a_cursor_at_the_mark_cannot_be_told() {
    let at_mark = format!("abc\x1b[{};{}H", MARK.0, MARK.1);
    let want = reference(at_mark.as_bytes(), ROWS, COLS);
    assert_eq!(judge("baud", want, want), Verdict::AtMark);
    assert!(!Verdict::AtMark.rated());
}

#[test]
fn vt_child_asks_the_questions_the_check_reads() {
    let script =
        std::fs::read_to_string(repo().join("tools/scripts/vt-child.sh")).expect("vt-child.sh");
    let printf = r"printf '\033[?2026l\033[6n\033[4242;1;1;1;%s;%s*y\033[13;37H\033[6n'";
    assert!(
        script.contains(printf),
        "vt-child.sh no longer sends {printf}"
    );
    // The same bytes, with the size put in, are what the check writes.
    let sent = printf
        .trim_start_matches("printf '")
        .trim_end_matches('\'')
        .replace(r"\033", "\x1b")
        .replacen("%s", &ROWS.to_string(), 1)
        .replacen("%s", &COLS.to_string(), 1);
    assert_eq!(sent.as_bytes(), questions(ROWS, COLS).as_slice());
}

#[test]
fn through_a_pty_baud_core_reads_the_same_screen() {
    let verdict = through_a_pty(b"");
    assert_eq!(verdict, Verdict::Same, "{}", verdict.says());
}

#[test]
fn through_a_pty_a_planted_wrong_cell_reads_unequal() {
    let verdict = through_a_pty(b"\x1b7\x1b[1;1HX\x1b8");
    assert!(
        matches!(verdict, Verdict::Unequal(Differs::Checksum(..))),
        "{}",
        verdict.says()
    );
}

#[test]
fn through_a_pty_a_moved_cursor_reads_unequal() {
    // The check every other terminal is held to: one cell, so it must
    // fire on its own.
    let verdict = through_a_pty(b"\x1b[20;70H");
    assert!(
        matches!(verdict, Verdict::Unequal(Differs::Cursor(..))),
        "{}",
        verdict.says()
    );
}

/// When this binary is run again as the child's `vt-reply`: reads the tty
/// it was given as stdin and writes `vt-reply`'s output to the file named
/// in `SCREEN_CHECK_REPLY_OUT`. A test run otherwise, and does nothing.
#[test]
fn as_vt_reply() {
    let Some(out) = std::env::var_os("SCREEN_CHECK_REPLY_OUT") else {
        return;
    };
    let args: Vec<String> = std::env::var("SCREEN_CHECK_REPLY_ARGS")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut text = Vec::new();
    vt_reply::run(&args, 0, &mut Live::new(), &mut text);
    std::fs::write(out, text).expect("the reply file");
}

/// The repository's root.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("baud-bench sits in the repository")
        .to_path_buf()
}

/// A fresh directory for one control.
fn scratch() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "screen-check-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// One read into `t`.
fn feed(t: &mut Term<'_>, chunk: &[u8]) {
    for c in chunk.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

/// `tools/scripts/vt-child.sh baud` replaying [`RECORDING`] once on a pty of
/// [`ROWS`] x [`COLS`], a headless baud-core at the other end answering it,
/// with `plant` fed to that baud-core just before the child's questions
/// (empty for none); the verdict `vt-screens` gives the line it wrote.
fn through_a_pty(plant: &[u8]) -> Verdict {
    let dir = scratch();
    let rec = dir.join("control.raw");
    std::fs::write(&rec, RECORDING).expect("the recording");
    let out = dir.join("results.txt");
    let exe = std::env::current_exe().expect("this test binary");
    let wrapper = dir.join("vt-reply");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nSCREEN_CHECK_REPLY_OUT=\"{d}/reply.$$\" SCREEN_CHECK_REPLY_ARGS=\"$*\" \"{e}\" --exact as_vt_reply --test-threads=1 >/dev/null 2>&1\ncat \"{d}/reply.$$\"\n",
            d = dir.display(),
            e = exe.display()
        ),
    )
    .expect("the wrapper");
    let status = std::process::Command::new("chmod")
        .arg("+x")
        .arg(&wrapper)
        .status()
        .expect("chmod");
    assert!(status.success());

    // The system's directories first, then this process's own PATH: a
    // system that keeps bash, perl and stty elsewhere finds them there.
    let mut path = "/usr/bin:/bin:/usr/sbin:/sbin".to_string();
    if let Some(own) = std::env::var_os("PATH").filter(|p| !p.is_empty()) {
        path.push(':');
        path.push_str(&own.to_string_lossy());
    }
    let mut env = Environ::current();
    env.set("HOME", &dir.to_string_lossy());
    env.set("PATH", &path);
    env.set("TERM", "xterm-256color");
    let argv: Vec<OsString> = [
        OsString::from("bash"),
        repo().join("tools/scripts/vt-child.sh").into_os_string(),
        "baud".into(),
        rec.clone().into_os_string(),
        "1".into(),
        out.clone().into_os_string(),
        wrapper.into_os_string(),
    ]
    .into();
    let spawn = Spawn::new(Lookup::Path, &argv, None, &env, 127).expect("the spawn");
    let child = Child::fork(&spawn).expect("the fork");
    child
        .set_size(i64::from(COLS), i64::from(ROWS), 0, 0)
        .expect("the size");

    let ws = WinSize {
        ws_row: ROWS,
        ws_col: COLS,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let trigger = b"\x1b[?2026l";
    let mut planted = plant.is_empty();
    let mut held = Vec::new();
    let start = std::time::Instant::now();
    while !child.exited() {
        if start.elapsed().as_secs() >= 60 {
            let _ = child.kill(libc::SIGKILL);
            child.reap();
            panic!("vt-child.sh did not finish within a minute");
        }
        if !child.readable(0.05).expect("select on the pty") {
            continue;
        }
        let Ok(got) = child.read(65536) else {
            break;
        };
        if got.is_empty() {
            break;
        }
        held.extend_from_slice(&got);
        if !planted {
            // The plant goes in just before the questions: everything up to
            // the update's end is fed, then the plant, then the rest.
            match held.windows(trigger.len()).position(|w| w == trigger) {
                Some(at) => {
                    feed(&mut t, &held[..at]);
                    feed(&mut t, plant);
                    planted = true;
                    held.drain(..at);
                }
                None => {
                    // Keep a tail that may be the trigger's start.
                    let keep = held.len().min(trigger.len() - 1);
                    let cut = held.len() - keep;
                    feed(&mut t, &held[..cut]);
                    held.drain(..cut);
                }
            }
        }
        if planted {
            feed(&mut t, &held);
            held.clear();
        }
        if !t.reply().is_empty() {
            child.write(t.reply()).expect("the reply to the child");
            t.clear_reply();
        }
    }
    assert_eq!(child.wait(), Ok(0), "vt-child.sh's exit");
    child.close();
    assert!(planted, "the child never ended a synchronised update");

    let line = std::fs::read_to_string(&out).expect("vt-child.sh wrote a result");
    let line = line.lines().last().expect("a result line");
    let (name, recording, verdict) =
        screen_check::judge_line(line, &[("control.raw".to_string(), RECORDING.to_vec())])
            .expect("the line reads");
    assert_eq!((name.as_str(), recording.as_str()), ("baud", "control.raw"));
    let _ = std::fs::remove_dir_all(&dir);
    verdict
}
