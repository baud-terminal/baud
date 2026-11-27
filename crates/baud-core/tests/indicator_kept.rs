//! **The indicator line is composed only when what it shows moved.**
//!
//! Every read's end draws the indicator status line (070-14 §14.2.1) from
//! the terminal's state, and a compose stores every cell of the row, which
//! stamps it: a renderer keeping the line's last copy reads it written and
//! lays it out again on every frame a read arrived in. The compose is
//! keyed on the fields it draws (`Indicators`), and skipped while the line
//! still shows them.
//!
//! **The key is the fields alone, and every other path to the line forgets
//! it.** The line's generation cannot be the key: a store stamps the
//! generation of the frame, and only a seal moves it, so a compose and any
//! later write to the line before the next seal stamp the same one. So each
//! control here takes no seal at all, and drives a writer of the line other
//! than the compose (a type change, the host-writable line, `RIS`, a
//! resize) inside one frame interval.
//!
//! **Each reference is one compose of the final state**: a second terminal
//! at the subject's final width, its host-call state set before it reads,
//! fed all of the subject's bytes as one read. A reference fed the same
//! reads would run the same key, and go wrong exactly as the subject does.
//! Where the final state needs a host call after a read (the locator's
//! report), the line is compared with the text its fields put at their
//! columns.
//!
//! **The source check** holds what no type in `baud-core` can:
//! `baud-domain`'s `SurfaceStore::stamped` and `split` hand out the line's
//! plane to any caller, so the places that call them on it are listed.

mod support;

use baud_core::*;
use std::fs;
use std::path::{Path, PathBuf};

const DECSSDT_1: &[u8] = b"\x1b[1$~";

/// Every lamp lit, CD and OH as read.
const LIT: IndicatorLamps = IndicatorLamps {
    cd: ReadLamp::Read(LampState::Lit),
    oh: ReadLamp::Read(LampState::Lit),
    cs: LampState::Lit,
    fax: LampState::Lit,
    syn: LampState::Lit,
};

fn ws(cols: u16) -> WinSize {
    WinSize {
        ws_row: 6,
        ws_col: cols,
    }
}

/// A terminal `cols` wide with a printer attached, so the printer field
/// has a port to show.
fn with<R>(cols: u16, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let w = ws(cols);
    let mut owned = Owned::new(&w).with_printer(PrintBudget::of_bytes(4096));
    let mut t = Term::new(owned.storage(), &w);
    f(&mut t)
}

/// The status line's text, its trailing blanks dropped.
fn line(t: &Term<'_>) -> String {
    t.status_line()
        .expect("a status line is shown")
        .cells
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// **The reference**: a terminal `cols` wide, `host` applied before it
/// reads, fed `bytes` as one read, so its line is the one compose of the
/// final state.
fn composed_once(cols: u16, host: impl FnOnce(&mut Term<'_>), bytes: &[u8]) -> String {
    with(cols, |t| {
        host(t);
        support::feed(t, bytes);
        line(t)
    })
}

/// A subject's line `got` against its reference `want`, with the control
/// that the change shows on the line at all: `before`, the subject's line
/// before it, is not `want`.
fn drawn(what: &str, before: &str, got: &str, want: &str) {
    assert_ne!(before, want, "{what}: the control, the change shows");
    assert_eq!(got, want, "{what}");
}

/// **The red: a read that moves nothing the line shows leaves it**, its
/// generation where the seal before the read left it. Eight such reads
/// each sealed before; then a read that moves the cursor, which draws it,
/// the control.
#[test]
fn a_read_that_moves_nothing_the_indicator_shows_leaves_its_line() {
    with(80, |t| {
        support::feed(t, DECSSDT_1);
        let mut moved = 0;
        for _ in 0..8 {
            let seal = t.seal_writes();
            support::feed(t, b"\x1b[m");
            if t.status_gen().expect("a status line is lent").after(&seal) {
                moved += 1;
            }
        }
        assert_eq!(
            moved, 0,
            "quiet reads that moved the indicator line: {moved} of 8"
        );
        let seal = t.seal_writes();
        support::feed(t, b"a");
        assert!(
            t.status_gen().expect("a status line is lent").after(&seal),
            "the control: a read that moves the cursor draws the line"
        );
        assert!(line(t).starts_with("    1(01,002)"), "{:?}", line(t));
    });
}

/// **1: the cursor moved by a read**, the position field.
#[test]
fn the_cursor_moved_by_a_read_is_drawn() {
    let bytes: [&[u8]; 2] = [DECSSDT_1, b"\x1b[5;17H"];
    let want = composed_once(80, |_| {}, &bytes.concat());
    with(80, |t| {
        support::feed(t, bytes[0]);
        let before = line(t);
        support::feed(t, bytes[1]);
        drawn("the cursor", &before, &line(t), &want);
    });
}

/// **2: KAM set by a read**, the Wait field.
#[test]
fn the_keyboard_locked_by_a_read_is_drawn() {
    let bytes: [&[u8]; 2] = [DECSSDT_1, b"\x1b[2h"];
    let want = composed_once(80, |_| {}, &bytes.concat());
    with(80, |t| {
        support::feed(t, bytes[0]);
        let before = line(t);
        support::feed(t, bytes[1]);
        drawn("KAM", &before, &line(t), &want);
    });
}

/// **3: the locator enabled by a read and moved by its host call**, the
/// position its report redraws, compared with the field at its column (23,
/// one-based 24) and the position before it.
#[test]
fn the_locator_moved_after_a_read_is_drawn() {
    with(80, |t| {
        t.set_locator_device(LocatorDevice::Present);
        support::feed(t, DECSSDT_1);
        support::feed(t, b"\x1b[2;2'z\x1b[1'{");
        let before = line(t);
        let at = Some(LocatorAt {
            col: Col::at_index(6),
            row: Row::from_index(2),
            px: None,
        });
        assert!(!t.locator(LocatorInput {
            kind: LocatorKind::Moved,
            at,
            held: LocatorButtons::NONE,
        }));
        let want = "    1(01,001)          [3,7]";
        let got = line(t);
        drawn("the locator", &before, got.get(..28).unwrap_or(&got), want);
    });
}

/// **4: the lamps set by their host call, nothing read**.
#[test]
fn the_lamps_set_with_nothing_read_are_drawn() {
    let want = composed_once(80, |t| t.set_indicator_lamps(Some(LIT)), DECSSDT_1);
    with(80, |t| {
        support::feed(t, DECSSDT_1);
        let before = line(t);
        t.set_indicator_lamps(Some(LIT));
        drawn("the lamps", &before, &line(t), &want);
    });
}

/// **5: the printer's state by its host call** (`Term::set_printer`).
#[test]
fn the_printer_set_with_nothing_read_is_drawn() {
    fn ready(t: &mut Term<'_>) {
        t.set_printer(PrinterPort::Ready);
    }
    let want = composed_once(80, ready, DECSSDT_1);
    with(80, |t| {
        support::feed(t, DECSSDT_1);
        let before = line(t);
        ready(t);
        drawn("the printer", &before, &line(t), &want);
    });
}

/// **6: a resize of the width with nothing read**, 80 columns to 50 with
/// the cursor at home, so no field the line shows moves: the printer
/// field, at column 41 and 13 wide, leaves a line narrower than 54
/// columns, and the resize blanks only the cells past the new width,
/// 50..80. A compose skipped here leaves `Printer: ` in 41..50.
#[test]
fn a_resize_with_nothing_read_draws_the_line_at_its_width() {
    let want = composed_once(50, |_| {}, DECSSDT_1);
    with(80, |t| {
        support::feed(t, DECSSDT_1);
        let before = line(t);
        assert_eq!(t.resize(&ws(50)), Resized::Ok);
        drawn("the resize", &before, &line(t), &want);
    });
}

/// The bytes of controls 7 and 7b after the line is shown: the host-writable
/// type, text written on it, and the indicator again, which blanks the line
/// at each change of type.
const AWAY_AND_BACK: [&[u8]; 3] = [b"\x1b[2$~\x1b[1$}", b"host", b"\x1b[0$}\x1b[1$~"];

/// **7: the type changed away and back inside one read**, the
/// host-writable line written between: DECSSDT 2, DECSASD 1, text,
/// DECSASD 0, DECSSDT 1. Nothing the indicator shows moved, the cursor
/// included, and the line was blanked twice: it is drawn, not left blank.
#[test]
fn the_indicator_after_the_host_line_in_one_read_is_drawn() {
    let want = composed_once(
        80,
        |_| {},
        &[&[DECSSDT_1][..], &AWAY_AND_BACK[..]].concat().concat(),
    );
    assert!(want.starts_with("    1(01,001)"), "the control: {want:?}");
    with(80, |t| {
        support::feed(t, DECSSDT_1);
        support::feed(t, &AWAY_AND_BACK.concat());
        assert_eq!(line(t), want);
    });
}

/// **7b: the same bytes over three reads**, the host-writable line taken
/// up at a read's start in the second.
#[test]
fn the_indicator_after_the_host_line_over_three_reads_is_drawn() {
    let want = composed_once(
        80,
        |_| {},
        &[&[DECSSDT_1][..], &AWAY_AND_BACK[..]].concat().concat(),
    );
    assert!(want.starts_with("    1(01,001)"), "the control: {want:?}");
    with(80, |t| {
        support::feed(t, DECSSDT_1);
        for read in AWAY_AND_BACK {
            support::feed(t, read);
        }
        assert_eq!(line(t), want);
    });
}

/// **8: `RIS` and the indicator again**, the cursor at home before, where
/// `RIS` puts it, the keyboard unlocked and the locator off: `RIS` erases
/// the line (070-14 p. 14-10) and DECSSDT 1 blanks it, and nothing the
/// indicator shows moved. It is drawn, not left blank.
#[test]
fn the_indicator_after_a_full_reset_is_drawn() {
    let want = composed_once(80, |_| {}, b"\x1b[1$~\x1bc\x1b[1$~");
    assert!(want.starts_with("    1(01,001)"), "the control: {want:?}");
    with(80, |t| {
        support::feed(t, DECSSDT_1);
        support::feed(t, b"\x1bc\x1b[1$~");
        assert_eq!(line(t), want);
    });
}

// ---- the source check ------------------------------------------------------

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// A file's code on one line: each line up to its `//`, every run of blanks
/// one space, and none before a `.`, so a call `rustfmt` laid over several
/// lines reads as it would on one.
fn code_of(src: &str) -> String {
    let mut c = String::new();
    for l in src.lines() {
        let code = l.split("//").next().unwrap_or("");
        c.push(' ');
        c.push_str(code);
    }
    let joined = c.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.replace(" .", ".")
}

/// The name of the `fn` enclosing byte `at` of `code`.
fn enclosing_fn(code: &str, at: usize) -> String {
    let Some(k) = code[..at].rfind(" fn ") else {
        return "?".into();
    };
    code[k + 4..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// **Every call that can reach the status line's plane** in `code` (the
/// file `file`), each as `file fn: call`: `stamped` with any argument but
/// the two screens' literals, the loop it is called in when its argument
/// is a variable, and the call after it; and `surfaces.split`, its index or not. The
/// literal calls are counted beside.
fn plane_calls(file: &str, code: &str) -> (Vec<String>, usize) {
    let (mut sites, mut literal) = (Vec::new(), 0);
    let mut from = 0;
    while let Some(k) = code[from..].find(".stamped(") {
        let at = from + k;
        let open = at + ".stamped(".len();
        let close = open + code[open..].find(')').expect("a closed call");
        from = close;
        let arg: String = code[open..close]
            .chars()
            .filter(|c| !c.is_whitespace() && *c != ',')
            .collect();
        if arg == "Which::Primary" || arg == "Which::Alternate" {
            literal += 1;
            continue;
        }
        let f = enclosing_fn(code, at);
        let site = match arg.starts_with("Which::") {
            true => format!("{file} fn {f}: stamped({arg})"),
            false => {
                let head = format!("for {arg} in [");
                let fn_at = code[..at].rfind(" fn ").unwrap_or(0);
                let over = code[fn_at..at]
                    .rfind(&head)
                    .map(|h| fn_at + h + head.len())
                    .and_then(|s| code[s..].find(']').map(|e| code[s..s + e].replace(' ', "")))
                    .unwrap_or_else(|| "?".into());
                let then = code[close + 1..]
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .replace(' ', "");
                format!("{file} fn {f}: stamped({arg}) over [{over}] then {then:?}")
            }
        };
        sites.push(site);
    }
    let mut from = 0;
    while let Some(k) = code[from..].find("surfaces.split") {
        let at = from + k;
        from = at + 1;
        sites.push(format!(
            "{file} fn {}: surfaces.split",
            enclosing_fn(code, at)
        ));
    }
    (sites, literal)
}

/// The calls that may reach the line's plane, each once: the host side's
/// accessor, which forgets the key as it hands out the line; the resize's
/// two loops, the line in only the one that stamps every row and writes no
/// cell; and the read's three borrows, which build `Paint::status` and
/// write nothing.
const PLANE_CALLS: [&str; 4] = [
    "status.rs fn status_plane: stamped(Which::Status)",
    "term.rs fn resized: stamped(w) over [Which::Primary,Which::Alternate] then \"\"",
    "term.rs fn resized: stamped(w) over [Which::Primary,Which::Alternate,Which::Status] then \".stamp_all()\"",
    "paint.rs fn new: surfaces.split",
];

/// **The status line's plane is reached only through its key**: every call
/// in `crates/baud-core/src` that can hand out the line's plane is one of
/// [`PLANE_CALLS`], each found once, so a new writer of the line that
/// bypassed the forgetting accessors, or a plane passed as a variable, fails
/// here. Positive controls: the scan finds the screens' literal calls, and
/// it reads a call laid over several lines, and a plane passed as a
/// variable, as sites the list does not hold.
#[test]
fn the_status_plane_is_reached_only_through_its_key() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files.sort();
    let (mut sites, mut literal) = (Vec::new(), 0);
    for f in &files {
        let name = f.file_name().and_then(|n| n.to_str()).expect("a file name");
        let src = fs::read_to_string(f).expect("a source file");
        let (s, l) = plane_calls(name, &code_of(&src));
        sites.extend(s);
        literal += l;
    }
    assert!(
        literal > 0,
        "the control: the scan finds the screens' calls"
    );
    let reflowed = "impl X {\n    fn sneak(&mut self) {\n        self\n            .store\n            .surfaces\n            .stamped(\n                Which::Status,\n            )\n            .stamp_all();\n    }\n}";
    assert_eq!(
        plane_calls("x.rs", &code_of(reflowed)).0,
        ["x.rs fn sneak: stamped(Which::Status)"],
        "the control: a call over several lines"
    );
    let passed =
        "fn sneak(&mut self, plane: Which) { self.store.surfaces.stamped(plane).stamp_all(); }";
    let (got, _) = plane_calls("x.rs", &code_of(passed));
    assert_eq!(got.len(), 1, "the control: a plane passed as a variable");
    assert!(!PLANE_CALLS.contains(&got[0].as_str()), "{got:?}");
    let mut want: Vec<String> = PLANE_CALLS.iter().map(|s| s.to_string()).collect();
    want.sort();
    sites.sort();
    assert_eq!(sites, want, "the calls that reach the status line's plane");
}
