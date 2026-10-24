//! DECRQTSR, DECTSR, DECCTR and DECRSTS: the terminal state report, the
//! colour table report and their restore.
//!
//! `[[source: dec-std-070]]` pp. 5-109 and 5-206 to 5-208, `[[source:
//! vt3xx-tp-002]]` pp. 205-209. DECTSR's data is baud's own layout, which
//! the text leaves to the firmware; what a test can hold is its shape --
//! `DCS 1 $ s`, nibbles `4/0..4/15`, two checksum bytes, 256 characters at
//! most -- and that a restore puts back what every other TSI control
//! reports.

use crate::support;
use baud_core::*;
use baud_domain::{TSR_FIXED, TSR_TAB_COLS};

fn show(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

fn reply(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    support::feed(t, bytes);
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

/// The data and checksum of a DECTSR, `D1 ... Dn c1 c2`.
fn data(report: &[u8]) -> &[u8] {
    report
        .strip_prefix(b"\x1bP1$s")
        .and_then(|r| r.strip_suffix(b"\x1b\\"))
        .unwrap_or_else(|| panic!("not a DECTSR: {}", show(report)))
}

/// The DECRSTS that restores `report`.
fn restore(report: &[u8]) -> Vec<u8> {
    [b"\x1bP1$p".as_slice(), data(report), b"\x1b\\"].concat()
}

/// The checksum bytes for `d`, as the report writes them.
fn check(d: &[u8]) -> [u8; 2] {
    let sum = d.iter().fold(0u8, |s, b| s.wrapping_add(*b));
    let c = 0u8.wrapping_sub(sum);
    [0x40 | (c >> 4), 0x40 | (c & 15)]
}

/// Every answer the discrete TSI controls give: DECRQM for each mode,
/// DECRQSS for each setting, DECRQPSR's two reports. A restore must put
/// all of them back.
fn tsi(t: &mut Term<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for m in [2, 4, 6, 8, 12, 20] {
        out.push(show(&reply(t, format!("\x1b[{m}$p").as_bytes())));
    }
    for m in baud_domain::SLOT_WIRE.into_iter().chain([1020, 1048]) {
        out.push(show(&reply(t, format!("\x1b[?{m}$p").as_bytes())));
    }
    for pt in [
        "m", "r", "s", " q", "\"q", " t", " u", "$|", "t", "*x", "\"p", "$~", "$}", ">t", ">0m",
        ">1m", ">2m", ">3m", ">4m", ">6m", ">7m", ">0f", ">1f", ">2f", ">3f", ">4f", ">6f", ">7f",
    ] {
        out.push(show(&reply(t, format!("\x1bP$q{pt}\x1b\\").as_bytes())));
    }
    out.push(show(&reply(t, b"\x1b[1$w")));
    out.push(show(&reply(t, b"\x1b[2$w")));
    out
}

/// A state far from power-on in every field the report carries.
const SET: &[u8] =
    b"\x1b[4h\x1b[20h\x1b[6h\x1b[8h\x1b[12l\x1b[2$~\x1b[?1h\x1b[?5h\x1b[?7l\x1b[?45h\x1b[?69h\x1b[?1004h\
\x1b[?1006h\x1b[?1002h\x1b[?2004h\x1b[?12l\x1b[?1036l\x1b[?66h\x1b[?67h\
\x1b[3;7r\x1b[2;30s\x1b[1;3;4:3;38;2;10;20;30;48;5;9;58;5;3;12m\x1b[5 q\
\x1b[1\"q\x1b[2 t\x1b[5 u\x1b[2*x\x1b[>0;2t\x1b[>1;3m\x1b[>4;2m\x1b[>2n\
\x1b[>4;1f\x1b(0\x1b)A\x1b*<\x1b+B\x0e\x1b[3g\x1bH\x1b[4;12H\x1bH\x1b7\x1b[6;9H";

/// **The report has STD 070's shape**: `DCS 1 $ s`, then data bytes in
/// `4/0..4/15` and two checksum bytes, the 2's complement of the 8-bit sum
/// of the data a nibble each, and 256 characters at most in all.
#[test]
fn decrqtsr_answers_a_report_in_std_070s_shape() {
    for ws in [
        support::CORPUS,
        WinSize {
            ws_row: 24,
            ws_col: 80,
        },
        WinSize {
            ws_row: 40,
            ws_col: 260,
        },
        WinSize {
            ws_row: 60,
            ws_col: 2048,
        },
    ] {
        let got = support::with_term(&ws, |t| {
            support::feed(t, SET);
            t.clear_reply();
            reply(t, b"\x1b[1$u")
        });
        assert!(
            got.len() <= 256,
            "{} bytes at {} columns",
            got.len(),
            ws.ws_col
        );
        let body = data(&got);
        assert!(body.iter().all(|b| b & 0xf0 == 0x40), "{}", show(body));
        let (d, c) = body.split_at(body.len() - 2);
        assert_eq!(c, check(d), "the checksum at {} columns", ws.ws_col);
    }
}

/// **The report is the layout's length**: the fixed fields, then the tab
/// bitmap a nibble for four columns where it fits, and the checksum,
/// with a `DECSC` save and without one; a screen too wide for the bitmap
/// carries none. At [`TSR_TAB_COLS`] columns with a save it is the whole
/// of STD 070's 256 characters.
#[test]
fn the_report_is_the_layouts_length() {
    let fit = u16::try_from(TSR_TAB_COLS).unwrap();
    for (cols, bitmap) in [(80, 20), (fit, TSR_TAB_COLS / 4), (2048, 0)] {
        let ws = WinSize {
            ws_row: 24,
            ws_col: cols,
        };
        for before in [&b""[..], b"\x1b[5;9H\x1b7"] {
            let got = support::with_term(&ws, |t| {
                support::feed(t, before);
                reply(t, b"\x1b[1$u")
            });
            assert_eq!(
                data(&got).len(),
                TSR_FIXED + bitmap + 2,
                "{cols} columns, {}",
                show(before)
            );
            assert!(got.len() <= 256);
        }
    }
}

/// "0 ignored, no report sent", and "If the terminal does not recognize
/// the type of DECTSR requested, the entire command is ignored" (p. 5-206).
#[test]
fn decrqtsr_ignores_any_other_report() {
    support::with_corpus(|t| {
        for q in [&b"\x1b[$u"[..], b"\x1b[0$u", b"\x1b[3$u", b"\x1b[2;3$u"] {
            assert_eq!(show(&reply(t, q)), "", "{}", show(q));
        }
    });
}

/// **A restore puts back every TSI answer**: the state is set, reported,
/// lost to `RIS`, and restored from the report, and DECRQM, DECRQSS and
/// DECRQPSR then answer as they did before.
#[test]
fn decrsts_restores_what_every_tsi_control_reports() {
    support::with_corpus(|t| {
        support::feed(t, SET);
        t.clear_reply();
        let before = tsi(t);
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1bc");
        assert_ne!(tsi(t), before, "RIS changed nothing the report carries");
        support::feed(t, &restore(&report));
        t.clear_reply();
        let after = tsi(t);
        let wrong: Vec<_> = before
            .iter()
            .zip(&after)
            .filter(|(b, a)| b != a)
            .map(|(b, a)| format!("{b} -> {a}"))
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    });
}

/// **A restore replays a direct colour in every slot**, through the SGR
/// DECRQSS answers with, `38:2::r:g:b` and its kin, which the parser reads
/// back with the colour space left empty: `SGR 0`, then the same rendition.
#[test]
fn decrsts_restores_a_direct_colour_in_every_slot() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[38;2;10;20;30;48;2;40;50;60;58;2;70;80;90m");
        let want = b"\x1bP1$r0;58:2::70:80:90;38:2::10:20:30;48:2::40:50:60m\x1b\\";
        t.clear_reply();
        assert_eq!(show(&reply(t, b"\x1bP$qm\x1b\\")), show(want));
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1bc");
        support::feed(t, &restore(&report));
        t.clear_reply();
        assert_eq!(show(&reply(t, b"\x1bP$qm\x1b\\")), show(want));
    });
}

/// **The level goes first**, since DECSCL soft-resets: a report taken at
/// Level 2 restores Level 2, and the modes after it.
#[test]
fn the_level_is_restored_before_what_it_resets() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[62;1\"p\x1b[?7l\x1b[3;5r");
        t.clear_reply();
        let before = tsi(t);
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1b[64;1\"p");
        support::feed(t, &restore(&report));
        t.clear_reply();
        assert_eq!(tsi(t), before);
    });
}

/// **A mode another gates is restored after it**: `?3` moves only while
/// `?40` is set, and the alternate screen is switched to only while
/// `?1046` is, so a report with both set restores both, whatever order the
/// slots run in.
#[test]
fn a_gated_mode_is_restored_after_its_gate() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?40h\x1b[?3h\x1b[?1049h");
        t.clear_reply();
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1bc\x1b[?1046l");
        t.clear_reply();
        assert_eq!(show(&reply(t, b"\x1b[?3$p")), "\\x1b[?3;2$y");
        support::feed(t, &restore(&report));
        t.clear_reply();
        assert_eq!(show(&reply(t, b"\x1b[?40$p")), "\\x1b[?40;1$y");
        assert_eq!(show(&reply(t, b"\x1b[?3$p")), "\\x1b[?3;1$y");
        assert_eq!(show(&reply(t, b"\x1b[?1049$p")), "\\x1b[?1049;1$y");
    });
}

/// **The page size is restored as DECSLPP restores it**: a report from a
/// 30-line page read into a 50-line one asks the embedder for 30 lines,
/// the request `CSI 30 t` makes.
#[test]
fn the_page_size_is_restored_as_a_window_request() {
    let short = WinSize {
        ws_row: 30,
        ws_col: 200,
    };
    let report = support::with_term(&short, |t| reply(t, b"\x1b[1$u"));
    let want = support::with_corpus(|t| {
        support::feed(t, b"\x1b[30t");
        t.window_requests().to_vec()
    });
    assert!(!want.is_empty());
    support::with_corpus(|t| {
        support::feed(t, &restore(&report));
        assert_eq!(t.window_requests(), want);
    });
}

/// **The C1 transmission is restored on its own when the level is the
/// same**, as `S8C1T` sets it, without the soft reset DECSCL would bring:
/// a report taken under `S8C1T` and restored under `S7C1T` answers in
/// eight-bit controls again, its margins kept.
#[test]
fn the_c1_transmission_is_restored_without_a_soft_reset() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b G");
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1b F\x1b[3;9r");
        let body = report
            .strip_prefix(b"\x901$s")
            .unwrap()
            .strip_suffix(b"\x9c")
            .unwrap();
        support::feed(t, &[b"\x1bP1$p".as_slice(), body, b"\x1b\\"].concat());
        t.clear_reply();
        assert_eq!(
            show(&reply(t, b"\x1bP$q\"p\x1b\\")),
            show(b"\x901$r64;0\"p\x9c")
        );
        // The report's margins were the screen's, so DECSTBM put them back.
        assert_eq!(
            show(&reply(t, b"\x1bP$qr\x1b\\")),
            show(b"\x901$r1;50r\x9c")
        );
    });
}

/// **The status line is restored before what lands on it**: a report taken
/// with a host-writable status line selected restores the type and then
/// the display, so DECRQSS answers both as they were.
#[test]
fn the_status_line_type_and_display_are_restored() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[2$~\x1b[1$}");
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1bc");
        support::feed(t, &restore(&report));
        t.clear_reply();
        assert_eq!(
            show(&reply(t, b"\x1bP$q$~\x1b\\")),
            show(b"\x1bP1$r2$~\x1b\\")
        );
        assert_eq!(
            show(&reply(t, b"\x1bP$q$}\x1b\\")),
            show(b"\x1bP1$r1$}\x1b\\")
        );
    });
}

/// "If the checksum fails, the entire sequence is ignored": one data byte
/// changed and the restore changes nothing. A report whose version is not
/// baud's, a byte outside `4/0..4/15`, and a cut body restore nothing
/// either.
#[test]
fn a_report_that_does_not_check_restores_nothing() {
    support::with_corpus(|t| {
        support::feed(t, SET);
        t.clear_reply();
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1bc");
        t.clear_reply();
        let reset = tsi(t);
        let body = data(&report).to_vec();
        let mut bad = body.clone();
        bad[5] ^= 1;
        // Version 1, the layout before `?1040` took the sixty-first slot.
        let mut foreign = body.clone();
        foreign[0] = 0x41;
        let n = foreign.len() - 2;
        let c = check(&foreign[..n]);
        foreign[n..].copy_from_slice(&c);
        let mut outside = body.clone();
        outside[7] = b'0';
        let cut = body[..body.len() - 3].to_vec();
        for d in [bad, foreign, outside, cut] {
            support::feed(t, &[b"\x1bP1$p".as_slice(), &d, b"\x1b\\"].concat());
            t.clear_reply();
            assert_eq!(tsi(t), reset, "{}", show(&d));
        }
    });
}

/// **A field baud cannot take is stepped over** (STD 070, "the terminal
/// will ignore the error if possible"): a cursor style past 6, its
/// checksum made good, leaves the style and restores the rest.
#[test]
fn an_invalid_field_is_stepped_over() {
    support::with_corpus(|t| {
        support::feed(t, SET);
        t.clear_reply();
        let before = tsi(t);
        let report = reply(t, b"\x1b[1$u");
        let mut d = data(&report).to_vec();
        // The version, the level, the ANSI modes, the private modes, the
        // rendition and the margins come before the cursor style.
        let at = 1 + 1 + 3 + 31 + 28 + 12;
        assert_eq!(d[at], 0x45, "the style nibble is DECSCUSR 5");
        d[at] = 0x49;
        let n = d.len() - 2;
        let c = check(&d[..n]);
        d[n..].copy_from_slice(&c);
        support::feed(t, b"\x1bc");
        support::feed(t, &[b"\x1bP1$p".as_slice(), &d, b"\x1b\\"].concat());
        t.clear_reply();
        let after = tsi(t);
        let differ: Vec<_> = before
            .iter()
            .zip(&after)
            .filter(|(b, a)| b != a)
            .map(|(b, _)| b.clone())
            .collect();
        assert_eq!(differ, ["\\x1bP1$r5 q\\x1b\\\\"], "only the style is left");
    });
}

/// **`DCS 1 $ p` and `DCS 2 $ p` are DECRSTS and not ReGIS**: the `$`
/// before the `p` keeps them out of the ReGIS interpreter, so a ReGIS
/// report in the body is not answered, and a bare `DCS 1 p` is ReGIS still.
#[test]
fn a_dollar_p_string_is_no_regis() {
    support::with_corpus(|t| {
        assert_eq!(show(&reply(t, b"\x1bP1$pP[10,20]R(P)\x1b\\")), "");
        assert_eq!(show(&reply(t, b"\x1bP2$pP[10,20]R(P)\x1b\\")), "");
        assert_eq!(show(&reply(t, b"\x1bP1pP[10,20]R(P)\x1b\\")), "[10,20]\\r");
    });
}

/// **The tab stops travel in the report where they fit**: a screen up to
/// [`TSR_TAB_COLS`] columns carries them a bit a column; a wider one carries them when
/// they are every eighth column, and not otherwise, where DECRSTS leaves
/// them as they are and DECTABSR is the way to save them.
#[test]
fn the_tab_stops_are_restored_where_the_report_holds_them() {
    let tabs = b"\x1b[3g\x1b[5G\x1bH\x1b[100G\x1bH\x1b[200G\x1bH\x1b[1G";
    let fit = u16::try_from(TSR_TAB_COLS).unwrap();
    for (cols, carried) in [(80, true), (fit, true), (fit + 8, false)] {
        let ws = WinSize {
            ws_row: 10,
            ws_col: cols,
        };
        support::with_term(&ws, |t| {
            support::feed(t, tabs);
            t.clear_reply();
            let before = reply(t, b"\x1b[2$w");
            let report = reply(t, b"\x1b[1$u");
            support::feed(t, b"\x1b[3g");
            support::feed(t, &restore(&report));
            t.clear_reply();
            let after = reply(t, b"\x1b[2$w");
            match carried {
                true => assert_eq!(show(&after), show(&before), "{cols}"),
                false => assert_eq!(show(&after), "\\x1bP2$u\\x1b\\\\", "{cols}"),
            }
        });
    }
    // Every eighth column is carried at any width.
    let ws = WinSize {
        ws_row: 10,
        ws_col: 2048,
    };
    support::with_term(&ws, |t| {
        let before = reply(t, b"\x1b[2$w");
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1b[3g");
        support::feed(t, &restore(&report));
        t.clear_reply();
        assert_eq!(show(&reply(t, b"\x1b[2$w")), show(&before));
    });
}

/// **The report and the restore are the same at every split.**
#[test]
fn the_state_reports_are_the_same_at_every_split() {
    let whole = support::with_corpus(|t| {
        support::feed(t, SET);
        t.clear_reply();
        reply(t, b"\x1b[1$u")
    });
    let prog = [b"\x1b[1$u".as_slice(), &restore(&whole), b"\x1b[2;2$u"].concat();
    let want = support::with_corpus(|t| {
        support::feed(t, SET);
        t.clear_reply();
        reply(t, &prog)
    });
    for cut in 1..prog.len() {
        let got = support::with_corpus(|t| {
            support::feed(t, SET);
            t.clear_reply();
            support::feed(t, &prog[..cut]);
            support::feed(t, &prog[cut..]);
            t.reply().to_vec()
        });
        assert_eq!(show(&got), show(&want), "cut at {cut}");
    }
}

/// Under `S8C1T` the reports open with `90` and end with `9C`, and DECRSTS
/// reads the eight-bit form.
#[test]
fn the_state_reports_follow_s8c1t() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b G");
        let got = reply(t, b"\x1b[1$u");
        assert!(
            got.starts_with(b"\x901$s") && got.ends_with(b"\x9c"),
            "{}",
            show(&got)
        );
        let got = reply(t, b"\x1b[2;2$u");
        assert_eq!(show(&got), show(b"\x902$s\x9c"));
    });
}

// ---- DECCTR -------------------------------------------------------------------

/// A palette of 256 entries, entry `i` at `(i, 255 - i, i / 2)`.
fn palette() -> [(u8, u8, u8); 256] {
    std::array::from_fn(|i| {
        let v = u8::try_from(i).unwrap();
        (v, 255 - v, v / 2)
    })
}

/// The colour table's groups, `Pc;Pu;Px;Py;Pz` each.
fn groups(report: &[u8]) -> Vec<String> {
    let body = report
        .strip_prefix(b"\x1bP2$s")
        .and_then(|r| r.strip_suffix(b"\x1b\\"))
        .unwrap_or_else(|| panic!("not a DECCTR: {}", show(report)));
    match body.is_empty() {
        true => Vec::new(),
        false => String::from_utf8(body.to_vec())
            .unwrap()
            .split('/')
            .map(str::to_string)
            .collect(),
    }
}

/// **Each colour the core knows is a group**, RGB in percent: entry 255,
/// `(255, 0, 127)`, is `255;2;100;0;50`. With no palette told, there is
/// nothing to report, as `OSC 4` answers nothing.
#[test]
fn decctr_reports_each_colour_the_core_knows() {
    support::with_corpus(|t| {
        assert_eq!(show(&reply(t, b"\x1b[2;2$u")), "\\x1bP2$s\\x1b\\\\");
        t.set_default_palette(&palette());
        let g = groups(&reply(t, b"\x1b[2;2$u"));
        assert_eq!(g.len(), 256);
        assert_eq!(g[0], "0;2;0;100;0");
        assert_eq!(g[255], "255;2;100;0;50");
        // An application's own colour is the one in use.
        support::feed(t, b"\x1b]4;7;rgb:ff/ff/ff\x1b\\");
        t.clear_reply();
        assert_eq!(groups(&reply(t, b"\x1b[2;2$u"))[7], "7;2;100;100;100");
    });
}

/// **`Pu` 0 or omitted is HLS**, the VT340's default (`[[source:
/// vt3xx-tp-002]]` p. 206): DEC's hue wheel, blue at 0, red at 120, green
/// at 240. The VT340's own map, Table 2-3 in RGB, reports its HLS within a
/// unit.
#[test]
fn decctr_reports_hls_by_default_on_decs_wheel() {
    let mut p = [(0u8, 0u8, 0u8); 256];
    // The VT340's colours 1-3, 20/20/79, 79/13/13, 20/79/20 percent.
    let pct = |v: u16| u8::try_from((v * 255 + 50) / 100).unwrap();
    p[1] = (pct(20), pct(20), pct(79));
    p[2] = (pct(79), pct(13), pct(13));
    p[3] = (pct(20), pct(79), pct(20));
    p[7] = (pct(46), pct(46), pct(46));
    support::with_corpus(|t| {
        t.set_default_palette(&p);
        for q in [&b"\x1b[2$u"[..], b"\x1b[2;0$u", b"\x1b[2;1$u"] {
            let g = groups(&reply(t, q));
            let hls = |i: usize| -> Vec<i32> {
                g[i].split(';')
                    .skip(1)
                    .map(|v| v.parse().unwrap())
                    .collect()
            };
            // The VT340's Table 2-3: 0/49/59, 120/46/71, 240/49/59, 0/46/0.
            for (i, want) in [
                (1, [1, 0, 49, 59]),
                (2, [1, 120, 46, 71]),
                (3, [1, 240, 49, 59]),
                (7, [1, 0, 46, 0]),
            ] {
                let got = hls(i);
                assert_eq!(got[0], 1, "{}", show(q));
                for k in 1..4 {
                    assert!(
                        (got[k] - want[k]).abs() <= 1,
                        "{i}: {got:?} against {want:?}"
                    );
                }
            }
        }
    });
}

/// **DECRSTS 2 sets the colours it lists, and only those**; a report read
/// back sets the colours that report the same again.
#[test]
fn decrsts_restores_the_colour_table() {
    support::with_corpus(|t| {
        t.set_default_palette(&palette());
        let report = reply(t, b"\x1b[2;2$u");
        support::feed(t, b"\x1b]4;1;rgb:00/00/00;2;rgb:00/00/00\x1b\\");
        t.clear_reply();
        assert_ne!(reply(t, b"\x1b[2;2$u"), report);
        let body = report.strip_prefix(b"\x1bP2$s").unwrap();
        support::feed(t, &[b"\x1bP2$p".as_slice(), body].concat());
        t.clear_reply();
        assert_eq!(show(&reply(t, b"\x1b[2;2$u")), show(&report));
        // One entry alone, in HLS: blue at hue 0.
        support::feed(t, b"\x1bP2$p9;1;0;50;100\x1b\\");
        t.clear_reply();
        assert_eq!(groups(&reply(t, b"\x1b[2;2$u"))[9], "9;2;0;0;100");
        assert_eq!(
            show(&reply(t, b"\x1b]4;9;?\x1b\\")),
            "\\x1b]4;9;rgb:0000/0000/ffff\\x1b\\\\"
        );
    });
}

/// **An invalid group stops the restore there** (the VT340's, VT420's and
/// VT510's "ignores the rest of the sequence"): the groups before it are
/// set, and none after.
#[test]
fn an_invalid_colour_group_stops_the_restore() {
    for bad in [
        "256;2;0;0;0",
        "4;3;0;0;0",
        "4;2;101;0;0",
        "4;1;361;0;0",
        "4;2;0;0",
        "4;2;;0;0",
    ] {
        support::with_corpus(|t| {
            t.set_default_palette(&palette());
            support::feed(
                t,
                format!("\x1bP2$p3;2;0;0;0/{bad}/5;2;0;0;0\x1b\\").as_bytes(),
            );
            t.clear_reply();
            let g = groups(&reply(t, b"\x1b[2;2$u"));
            assert_eq!(g[3], "3;2;0;0;0", "{bad}");
            assert_ne!(g[5], "5;2;0;0;0", "{bad}");
        });
    }
}

/// **A `$` kept for DECRSTS does not reach the escape that cancels its
/// string**: `DCS $ ESC D` and `DCS 1 $ p 4 ESC D` are a cancelled string
/// and then IND, as `ESC D` alone is, and not `ESC $ D`.
#[test]
fn a_cancelled_dollar_string_leaves_no_intermediate() {
    for cancel in [&b"\x1bP$\x1bD"[..], b"\x1bP1$p4\x1bD", b"\x1bP$qm\x1bD"] {
        support::with_corpus(|t| {
            let before = t.unsupported;
            support::feed(t, cancel);
            assert_eq!(t.cursor().0.index(), 1, "{}", show(cancel));
            assert_eq!(t.unsupported, before, "{}", show(cancel));
        });
    }
}

/// **The input encoding travels in the report**: a report taken under
/// `ESC % @` restores ISO 8859-1 after `ESC % G`, as `ESC % @` selects it,
/// and the bytes after the restore in the same read are decoded under it:
/// `C3 A9` is two characters there, and one, `é`, under UTF-8.
#[test]
fn the_input_encoding_is_restored() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b%@");
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1b%G");
        assert_eq!(t.input_encoding(), InputEncoding::Utf8);
        support::feed(
            t,
            &[restore(&report).as_slice(), b"\x1b[H\xc3\xa9"].concat(),
        );
        t.clear_reply();
        assert_eq!(t.input_encoding(), InputEncoding::Latin1);
        assert_eq!(show(&reply(t, b"\x1b[?1020$p")), "\\x1b[?1020;2$y");
        let first = support::screen(t)[0]
            .content
            .as_codepoint()
            .map(|c| c.value());
        assert_eq!(first, Some(0xc3), "decoded under ISO 8859-1");
        assert_eq!(t.cursor().1, Col::at_index(2));
    });
}

/// What `DECRC` gives back: the cursor information, the rendition, the
/// protection and the two modes it restores.
fn after_decrc(t: &mut Term<'_>) -> Vec<String> {
    support::feed(t, b"\x1b8");
    t.clear_reply();
    let mut out = vec![show(&reply(t, b"\x1b[1$w"))];
    for pt in ["m", "\"q"] {
        out.push(show(&reply(t, format!("\x1bP$q{pt}\x1b\\").as_bytes())));
    }
    for m in [6, 7] {
        out.push(show(&reply(t, format!("\x1b[?{m}$p").as_bytes())));
    }
    out
}

/// **The shown screen's `DECSC` save travels in the report** (Jev's lean,
/// S_SHOWN, 0.86/0.88 after research): a save lost to `RIS` comes back,
/// so `DECRC` after the restore returns to it, and `?1048` answers it is
/// saved; a report taken with nothing saved empties a save made since.
#[test]
fn the_saved_cursor_is_restored() {
    support::with_corpus(|t| {
        support::feed(
            t,
            b"\x1b[3;20r\x1b[?6h\x1b[?7l\x1b[5;9H\x1b[1;4;38;2;1;2;3;48;5;9m\x1b[1\"q\
\x1b(0\x1b)A\x1b*%5\x0e\x1b7",
        );
        let want = after_decrc(t);
        support::feed(t, b"\x1b[?6l\x1b[?7h\x1b[0m\x1b[0\"q\x1b(B\x0f\x1b[2;2H");
        assert_eq!(show(&reply(t, b"\x1b[?1048$p")), "\\x1b[?1048;1$y");
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1bc");
        assert_eq!(show(&reply(t, b"\x1b[?1048$p")), "\\x1b[?1048;2$y");
        support::feed(t, &restore(&report));
        t.clear_reply();
        assert_eq!(show(&reply(t, b"\x1b[?1048$p")), "\\x1b[?1048;1$y");
        assert_eq!(after_decrc(t), want);
    });
    support::with_corpus(|t| {
        let report = reply(t, b"\x1b[1$u");
        support::feed(t, b"\x1b[5;9H\x1b7\x1b[H");
        support::feed(t, &restore(&report));
        t.clear_reply();
        assert_eq!(show(&reply(t, b"\x1b[?1048$p")), "\\x1b[?1048;2$y");
    });
}

/// The index of DECCIR's `Sflag` nibble in a DECTSR's data, found rather
/// than counted: two reports that differ only in a pending wrap, Sflag's
/// bit 4, differ in that one nibble and nowhere else.
fn sflag_at() -> usize {
    let at = |program: &[u8]| {
        support::with_corpus(|t| {
            support::feed(t, program);
            t.clear_reply();
            data(&reply(t, b"\x1b[1$u")).to_vec()
        })
    };
    let (plain, pending) = (at(b"\x1b[1;200H"), at(b"\x1b[1;200Hx"));
    let n = plain.len() - 2;
    let differ: Vec<usize> = (0..n).filter(|&i| plain[i] != pending[i]).collect();
    assert_eq!(differ.len(), 1, "one nibble apart: {differ:?}");
    assert_eq!(pending[differ[0]] ^ plain[differ[0]], 8, "bit 4");
    differ[0]
}

/// What a held DECRSTS leaves: the width each held rest was fed at, the
/// width after, and the first two cells at the cursor the report placed.
#[derive(PartialEq, Eq, Debug)]
struct Landed {
    holds: Vec<Cols>,
    cols: Cols,
    cells: [char; 2],
}

/// `program` fed in reads cut at `cuts` to an 80-column terminal with
/// room for 132, under `hold`; honoured, each held rest is fed once the
/// grid has the width asked for, and a rest that holds again more than
/// four times fails rather than looping.
fn landed(hold: WidthHold, program: &[u8], cuts: &[usize]) -> Landed {
    let narrow = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    let room = WinSize {
        ws_row: 24,
        ws_col: 132,
    };
    let mut s = Owned::new(&room);
    let mut t = Term::new(s.storage(), &narrow);
    t.set_width_hold(hold);
    let mut holds = Vec::new();
    let mut from = 0;
    for &to in cuts.iter().chain(core::iter::once(&program.len())) {
        let chunk = &program[from..to];
        from = to;
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        while let Some(rest) = t.take_held() {
            assert!(holds.len() < 4, "a held rest held again: {cuts:?}");
            let ws = WinSize {
                ws_row: 24,
                ws_col: t.requested_width().expect("a held read asked").ws_col(),
            };
            assert_eq!(t.resize(&ws), Resized::Ok, "the room is provisioned");
            holds.push(t.geometry().0);
            t.feed(rest.at, rest.len);
        }
    }
    let row = t.row(Row::from_index(2)).expect("row 3");
    let ch = |x: usize| {
        row[x]
            .content
            .as_codepoint()
            .and_then(|c| char::from_u32(c.value()))
            .unwrap_or(' ')
    };
    Landed {
        holds,
        cols: t.geometry().0,
        cells: [ch(4), ch(5)],
    }
}

/// **A DECRSTS that asks for a page width and restores a pending single
/// shift holds the read, and the shift waits for the held rest.** A
/// report taken on a 132-column page, with line drawing in G2 or G3 and
/// the cursor at row 3, column 5, is restored on an 80-column one with
/// `SS2` or `SS3` pending in its DECCIR flags (Sflag bits 2 and 3, "SS2
/// pending (1 if SS2 received)", `[[source: dec-std-070]]` p. 5-201).
/// baud's own report
/// never carries one, as a DECRQTSR is itself a control; a host may send a
/// report baud did not make, since the checksum is the sender's. DECRSTS
/// (p. 5-208) restores "the state encoded in the data" and gives no order;
/// SS2 (070-3 p. 3-80) "causes the next character in the received data
/// stream ... to be rendered according to the current contents of the G2
/// set", and the next character is the held rest's first, which lands at
/// the 132 columns DECSCPP asked for. Without the shift the same restore
/// holds as well, the control; unheld the shift still applies at 80.
#[test]
fn a_restore_that_asks_a_width_holds_with_a_single_shift_pending() {
    let wide = WinSize {
        ws_row: 24,
        ws_col: 132,
    };
    // A report with line drawing designated by `desig`, and the same with
    // Sflag's `bit` set and the checksum made again.
    let report = |desig: &[u8], bit: u8| {
        let report = support::with_term(&wide, |t| {
            support::feed(t, desig);
            support::feed(t, b"\x1b[3;5H");
            t.clear_reply();
            reply(t, b"\x1b[1$u")
        });
        let mut d = data(&report).to_vec();
        let n = d.len() - 2;
        d[sflag_at()] |= bit;
        let c = check(&d[..n]);
        d[n..].copy_from_slice(&c);
        d
    };
    let wrap = |d: &[u8]| [b"\x1bP1$p".as_slice(), d, b"\x1b\\qq"].concat();
    let at_132 = vec![Cols::of_index(132)];
    let line = '\u{2500}';
    for (d, cells) in [
        (report(b"\x1b*0", 0), ['q', 'q']),
        (report(b"\x1b*0", 2), [line, 'q']),
        (report(b"\x1b+0", 4), [line, 'q']),
    ] {
        let program = wrap(&d);
        assert_eq!(
            landed(WidthHold::Off, &program, &[]),
            Landed {
                holds: Vec::new(),
                cols: Cols::of_index(80),
                cells,
            },
            "the control, unheld"
        );
        let want = Landed {
            holds: at_132.clone(),
            cols: Cols::of_index(132),
            cells,
        };
        assert_eq!(landed(WidthHold::On, &program, &[]), want, "whole");
        for size in 1..=program.len() {
            let cuts: Vec<usize> = (size..program.len()).step_by(size).collect();
            assert_eq!(
                landed(WidthHold::On, &program, &cuts),
                want,
                "read size {size}"
            );
        }
        for cut in 1..program.len() {
            assert_eq!(
                landed(WidthHold::On, &program, &[cut]),
                want,
                "split at {cut}"
            );
        }
    }
}
