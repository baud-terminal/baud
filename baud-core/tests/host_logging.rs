//! xterm's logging, `?46` and `OSC 46` (`[[source: ctlseqs]]`, "Start
//! logging (XTLOGGING)", "Change Log File to Pt"), the owner's call of
//! 2026-10-20 (note AV): the core keeps the mode and the name and stops a
//! read at each change, and the embedder writes the bytes.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 4,
};

/// One read fed as an embedder feeds it, the rest of a held read after
/// it: each part, with the mode it was parsed under.
fn parts(t: &mut Term<'_>, bytes: &[u8]) -> Vec<(Vec<u8>, LogMode)> {
    t.write_at(BytePos::ZERO, bytes);
    let mut out = Vec::new();
    let mut next = Some(Rest {
        at: BytePos::ZERO,
        len: ReadLen::from_index(bytes.len()),
    });
    while let Some(r) = next {
        let mode = t.log_mode();
        t.feed(r.at, r.len);
        next = t.take_held();
        let end = next.map_or(r.at.index() + r.len.index(), |n| n.at.index());
        out.push((bytes[r.at.index()..end].to_vec(), mode));
    }
    out
}

fn first_row(t: &Term<'_>) -> String {
    let row: String = t
        .row(Row::from_index(0))
        .expect("a row")
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect();
    row.trim_end().to_string()
}

/// What an embedder would write: the parts parsed while started.
fn logged(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    parts(t, bytes)
        .into_iter()
        .filter(|(_, m)| *m == LogMode::Started)
        .flat_map(|(b, _)| b)
        .collect()
}

fn decrqm(t: &mut Term<'_>) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, b"\x1b[?46$p");
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

/// **A core no embedder told refuses both, counts them, and answers `4`**:
/// it writes no file, so no program can set the mode (Jev,
/// PERMANENTLY_RESET, 0.96/1.00).
#[test]
fn off_both_are_refused_and_counted() {
    support::with_term(&WS, |t| {
        assert_eq!(t.logging(), Logging::Off);
        let refused = t.logs_refused();
        support::feed(t, b"\x1b]46;/tmp/x\x07\x1b[?46h");
        assert_eq!(t.log_mode(), LogMode::Stopped);
        assert_eq!(t.log_file(), None);
        assert_ne!(t.logs_refused(), refused);
        assert_eq!(decrqm(t), b"\x1b[?46;4$y");
        assert_eq!(t.take_held(), None, "no read stops");
    });
}

/// **The log is the host's bytes from the one after `?46 h` up to and
/// including `?46 l`**, xterm's `logstart = VTbuffer->next` and its flush
/// at `CloseLog` (`[[source: xterm-misc]]`): the read stops after each
/// change, so the part before it is parsed under the old mode and the
/// rest under the new.
#[test]
fn the_log_runs_from_after_the_start_to_the_stop() {
    support::with_term(&WS, |t| {
        t.set_logging(Logging::On);
        let p = parts(t, b"ab\x1b[?46hcd\x1b[?46lef");
        assert_eq!(
            p,
            [
                (b"ab\x1b[?46h".to_vec(), LogMode::Stopped),
                (b"cd\x1b[?46l".to_vec(), LogMode::Started),
                (b"ef".to_vec(), LogMode::Stopped),
            ]
        );
        assert_eq!(first_row(t), "abcdef", "all parsed");
        assert_eq!(
            logged(t, b"\x1b[?46hx\x1b[?46h"),
            b"x\x1b[?46h",
            "the start is not logged, and a second one is bytes"
        );
        assert_eq!(logged(t, b"yz"), b"yz", "nor held twice");
        assert_eq!(logged(t, b"\x1b[?46l"), b"\x1b[?46l");
        assert_eq!(
            logged(t, b"\x1b[?46lq"),
            b"",
            "a stop when stopped stops nothing"
        );
    });
}

/// **`DECRQM ?46` answers the mode**, `1` or `2`, with logging on.
#[test]
fn decrqm_answers_the_mode() {
    support::with_term(&WS, |t| {
        t.set_logging(Logging::On);
        assert_eq!(decrqm(t), b"\x1b[?46;2$y");
        let _ = parts(t, b"\x1b[?46h");
        assert_eq!(decrqm(t), b"\x1b[?46;1$y");
        t.stop_log();
        assert_eq!(decrqm(t), b"\x1b[?46;2$y", "the embedder could not log");
        let _ = parts(t, b"\x1b[?46h");
        t.set_logging(Logging::Off);
        assert_eq!(t.log_mode(), LogMode::Stopped, "turned off, it stops");
        assert_eq!(decrqm(t), b"\x1b[?46;4$y");
    });
}

/// **`OSC 46` names the file and only renames**: a log already started
/// keeps its mode; `?`, an empty `Pt` and a name `OSC_MAX` may have cut
/// change nothing and are counted, as xterm rings at `?` and drops an
/// empty body.
#[test]
fn osc_46_names_the_file_the_next_start_opens() {
    support::with_term(&WS, |t| {
        t.set_logging(Logging::On);
        assert_eq!(t.log_file(), None, "the embedder's own name");
        support::feed(t, b"\x1b]46;/tmp/a.log\x1b\\");
        assert_eq!(t.log_file(), Some(&b"/tmp/a.log"[..]));
        let _ = parts(t, b"\x1b[?46h\x1b]46;b.log\x07");
        assert_eq!(t.log_mode(), LogMode::Started, "renaming stops nothing");
        assert_eq!(t.log_file(), Some(&b"b.log"[..]));
        for bad in [&b"\x1b]46;?\x07"[..], b"\x1b]46;\x07", b"\x1b]46\x07"] {
            let refused = t.logs_refused();
            support::feed(t, bad);
            assert_ne!(t.logs_refused(), refused, "{bad:?}");
            assert_eq!(t.log_file(), Some(&b"b.log"[..]), "{bad:?}");
        }
        let mut long = b"\x1b]46;/".to_vec();
        long.extend(std::iter::repeat_n(b'x', OSC_MAX));
        long.push(0x07);
        let refused = t.logs_refused();
        support::feed(t, &long);
        assert_ne!(t.logs_refused(), refused, "a cut name");
        assert_eq!(t.log_file(), Some(&b"b.log"[..]));
    });
}

/// **A name beginning `|` is a command, and no start runs one**: xterm
/// runs it only when built with `ALLOWLOGFILEEXEC` and otherwise rings
/// and logs nothing (`StartLog`, `[[source: xterm-misc]]`; Jev,
/// XTERM_DEFAULT_BUILD, 1.00/1.00).
#[test]
fn a_command_is_never_started() {
    support::with_term(&WS, |t| {
        t.set_logging(Logging::On);
        support::feed(t, b"\x1b]46;|rm -rf ~\x07");
        let refused = t.logs_refused();
        let p = parts(t, b"\x1b[?46hx");
        assert_eq!(t.log_mode(), LogMode::Stopped);
        assert_ne!(t.logs_refused(), refused);
        assert_eq!(p.len(), 1, "no read stops");
    });
}

/// **XTSAVE keeps the mode and XTRESTORE starts or stops through `?46`'s
/// own path**, as xterm's `restoremodes` calls `StartLog` and `CloseLog`.
#[test]
fn xtsave_and_xtrestore_keep_the_mode() {
    support::with_term(&WS, |t| {
        t.set_logging(Logging::On);
        let _ = parts(t, b"\x1b[?46h\x1b[?46s\x1b[?46l");
        assert_eq!(t.log_mode(), LogMode::Stopped);
        let p = parts(t, b"\x1b[?46rab");
        assert_eq!(t.log_mode(), LogMode::Started);
        assert_eq!(
            p,
            [
                (b"\x1b[?46r".to_vec(), LogMode::Stopped),
                (b"ab".to_vec(), LogMode::Started),
            ]
        );
    });
}

/// **`RIS` and `DECSTR` leave the log and its name**, as xterm's resets do
/// (Jev, RIS_KEEPS, 0.98/0.96), and leave the setting.
#[test]
fn a_reset_leaves_the_log() {
    support::with_term(&WS, |t| {
        t.set_logging(Logging::On);
        support::feed(t, b"\x1b]46;keep.log\x07");
        let _ = parts(t, b"\x1b[?46h");
        assert_eq!(logged(t, b"\x1bc\x1b[!pz"), b"\x1bc\x1b[!pz");
        assert_eq!(t.log_mode(), LogMode::Started);
        assert_eq!(t.log_file(), Some(&b"keep.log"[..]));
        assert_eq!(t.logging(), Logging::On);
    });
}
