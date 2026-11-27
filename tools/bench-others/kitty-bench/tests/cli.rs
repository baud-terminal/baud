//! The binary's exit codes and streams, with no kitty on the machine.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("kitty-bench-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(args: &[&str], kitty_root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kitty-bench"))
        .args(args)
        .env("KITTY_ROOT", kitty_root)
        .env_remove("KITTY_LIBPYTHON")
        .output()
        .expect("the harness runs")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn no_argument_prints_usage_to_stderr_and_exits_one() {
    let d = Dir::new("usage");
    let o = harness(&[], &d.0);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).starts_with("usage: "), "{}", stderr(&o));
    assert!(stderr(&o).contains("<payload-dir>"), "{}", stderr(&o));
    assert_eq!(stdout(&o), "");
}

#[test]
fn a_directory_with_no_payloads_exits_one_with_no_table() {
    let d = Dir::new("nopayloads");
    let o = harness(&[d.0.to_str().unwrap()], &d.0);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("stream.bin"), "{}", stderr(&o));
    assert_eq!(stdout(&o), "");
}

#[test]
fn a_flag_is_taken_for_a_directory_and_fails_the_same_way() {
    let d = Dir::new("flag");
    let o = harness(&["--help"], &d.0);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("--help/stream.bin"), "{}", stderr(&o));
}

#[test]
fn an_empty_payload_exits_one_rather_than_hanging() {
    let d = Dir::new("emptypayload");
    std::fs::write(d.0.join("stream.bin"), b"").unwrap();
    std::fs::write(d.0.join("tui.bin"), b"x").unwrap();
    let o = harness(&[d.0.to_str().unwrap()], &d.0);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("empty payload"), "{}", stderr(&o));
}

#[test]
fn a_named_kitty_that_is_not_there_exits_one_before_any_measurement() {
    let d = Dir::new("nokitty");
    std::fs::write(d.0.join("stream.bin"), b"x").unwrap();
    std::fs::write(d.0.join("tui.bin"), b"x").unwrap();
    let root = d.0.join("not-a-kitty");
    let o = harness(&[d.0.to_str().unwrap(), "ignored"], &root);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("KITTY_ROOT"), "{}", stderr(&o));
    assert_eq!(stdout(&o), "");
}

/// The whole run up to the load: the ceiling row is a number because it is one,
/// and no kitty row follows when the extension will not load.
#[test]
fn an_extension_that_will_not_load_exits_one_with_no_kitty_row() {
    let d = Dir::new("badso");
    // Long enough, against 32 MiB, that the writer is still sending when the
    // reader stops: 68 copies, 445,568 bytes past.
    std::fs::write(d.0.join("stream.bin"), vec![b'x'; 500_000]).unwrap();
    std::fs::write(d.0.join("tui.bin"), vec![b'y'; 500_000]).unwrap();
    std::fs::create_dir_all(d.0.join("root/kitty")).unwrap();
    std::fs::write(d.0.join("root/kitty/fast_data_types.so"), b"not a library").unwrap();
    let o = harness(&[d.0.to_str().unwrap()], &d.0.join("root"));
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("cannot load"), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("ceiling (discard, no parse)"), "{out}");
    assert!(!out.contains("kitty (Screen.test_*)"), "{out}");
}
