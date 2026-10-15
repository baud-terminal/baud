//! XTSAVE `CSI ? Pm s` and XTRESTORE `CSI ? Pm r`.
//!
//! `[[source: ctlseqs]]`: "Save DEC Private Mode Values (XTSAVE), xterm. Ps
//! values are the same as for DECSET. Like Save Cursor (DECSC), this uses a
//! one-level cache. Unlike Save Cursor, specific settings can be saved and
//! restored independently. Only those modes listed as parameters are saved."
//! And "Restore DEC Private Mode Values (XTRESTORE), xterm. The value of Ps
//! previously saved is restored. Ps values are the same as for DECSET. ...
//! Only those modes listed as parameters are restored."
//!
//! So every mode DECSET acts on is saved as the value `DECRQM` answers, and a
//! restore puts each listed mode back through DECSET's own path -- with every
//! side effect setting or resetting that mode has, and only where its value
//! differs from the saved one. A mode never saved has no "value previously
//! saved", and restores nothing. A mode DECSET does not act on is counted, as
//! DECSET counts it.
//!
//! xterm's `savemodes` and `restoremodes` (`[[source: xterm-reset]]`) depart
//! from that text in places. Where Jev decided for the text on 2026-10-10,
//! each listed for the owner's veto, baud follows the text: xterm ignores
//! `?2` and `?8` and saves `?3` only under the `c132` resource `?40` sets
//! (XT-3), restores `?1049` without the cursor or the clear (XT-6), and
//! keeps the cache across `RIS` (XT-8); and where the text is silent, xterm
//! re-applies `?6`, and `?3` under `c132`, at a value already in force, and
//! baud changes nothing (XT-7). The rest are the owner's call rather than
//! followed here: it saves one tracking level under all four mouse modes and
//! one encoding under all four encodings, and restores a never-saved mode
//! from a zeroed array.

use crate::support;
use baud_core::*;
use baud_domain::{CacheGeometry, Depth};

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn screen_text(t: &Term<'_>) -> Vec<String> {
    (0..t.geometry().1.index())
        .map(|y| text_of(t.row(Row::from_index(y)).expect("a row on the grid")))
        .collect()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    support::feed(t, bytes);
}

fn feed_str(t: &mut Term<'_>, s: &str) {
    support::feed(t, s.as_bytes());
}

/// What `DECRQM` answers for private mode `mode`: 0 not recognised, 1 set,
/// 2 reset, 3 and 4 permanent.
fn ask(t: &mut Term<'_>, mode: u16) -> u16 {
    t.clear_reply();
    feed_str(t, &format!("\x1b[?{mode}$p"));
    let reply = String::from_utf8(t.reply().to_vec()).expect("an ASCII reply");
    t.clear_reply();
    let want = format!("\x1b[?{mode};");
    let rest = reply
        .strip_prefix(&want)
        .unwrap_or_else(|| panic!("DECRQM ?{mode} answered {reply:?}"));
    rest.strip_suffix("$y")
        .and_then(|ps| ps.parse().ok())
        .unwrap_or_else(|| panic!("DECRQM ?{mode} answered {reply:?}"))
}

/// The DECSET or DECRST that changes `mode` from what `DECRQM` says it is.
fn flip(t: &mut Term<'_>, mode: u16) {
    let fin = match ask(t, mode) {
        1 => 'l',
        _ => 'h',
    };
    feed_str(t, &format!("\x1b[?{mode}{fin}"));
}

/// **What XTSAVE saved, XTRESTORE puts back**, and `DECRQM` says so: a
/// sample across the kinds of mode -- a write mode, the cursor, an input
/// mode, a mouse level and an encoding, the blink, the wheel's and origin.
#[test]
fn xtrestore_brings_back_what_xtsave_saved() {
    for mode in [7u16, 25, 2004, 1000, 1006, 1, 12, 1007, 6, 1004] {
        support::with_corpus(|t| {
            let was = ask(t, mode);
            feed_str(t, &format!("\x1b[?{mode}s"));
            flip(t, mode);
            assert_ne!(ask(t, mode), was, "?{mode}: the control, the flip took");
            let counted = (t.unsupported, t.unsupported_param);
            feed_str(t, &format!("\x1b[?{mode}r"));
            assert_eq!(ask(t, mode), was, "?{mode}: restored, and DECRQM says so");
            assert_eq!(
                (t.unsupported, t.unsupported_param),
                counted,
                "?{mode}: acted on, not counted"
            );
        });
    }
    // And through the accessors an embedder reads, not only DECRQM.
    support::with_corpus(|t| {
        feed(t, b"\x1b[?25;2004;1000s\x1b[?25l\x1b[?2004h\x1b[?1000h");
        feed(t, b"\x1b[?25;2004;1000r");
        assert!(t.cursor_visible());
        assert!(!t.bracketed_paste());
        assert_eq!(t.mouse_tracking(), MouseTracking::Off);
    });
}

/// **A restore is DECSET, side effects and all.** Restoring `?1049` reset
/// leaves the alternate screen and restores the cursor `?1049 h` saved, as
/// `?1049 l` does; restoring it set saves the cursor, switches and clears.
/// Restoring `?47` switches and does nothing else, as `?47` does. xterm's
/// restore of `?1049` is the bare switch of `?47`; baud follows the text,
/// decided 2026-10-10 by Jev at 0.98 and 0.96 (XT-6), listed for the owner's
/// veto.
#[test]
fn xtrestore_has_the_side_effects_of_setting_the_mode() {
    support::with_corpus(|t| {
        feed(t, b"primary\x1b[5;5H");
        feed(t, b"\x1b[?1049s\x1b[?1049h\x1b[10;10Halt");
        assert_eq!(t.screen(), Screen::Alternate, "the control");
        feed(t, b"\x1b[?1049r");
        assert_eq!(t.screen(), Screen::Normal, "?1049 restored reset leaves");
        assert_eq!(
            text_of(t.row(Row::ZERO).expect("a row on the grid")),
            "primary"
        );
        assert_eq!(
            t.cursor(),
            (Row::from_index(4), Col::at_index(4)),
            "and restores the cursor ?1049 h saved"
        );
    });
    support::with_corpus(|t| {
        feed(
            t,
            b"\x1b[?1049h\x1b[3;3Hstale\x1b[?1049s\x1b[?1049l\x1b[2;2H",
        );
        assert_eq!(t.screen(), Screen::Normal, "the control");
        feed(t, b"\x1b[?1049r");
        assert_eq!(t.screen(), Screen::Alternate, "?1049 restored set enters");
        assert!(
            screen_text(t).iter().all(|l| l.is_empty()),
            "and clears the alternate screen"
        );
        feed(t, b"\x1b[?1049l");
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(1)),
            "the cursor it saved comes back on leaving"
        );
    });
    support::with_corpus(|t| {
        feed(t, b"\x1b[?47s\x1b[?47h\x1b[6;6H");
        feed(t, b"\x1b[?47r");
        assert_eq!(t.screen(), Screen::Normal, "?47 restored reset leaves");
        assert_eq!(
            t.cursor(),
            (Row::from_index(5), Col::at_index(5)),
            "and moves no cursor, as ?47 l does not"
        );
    });
    // DECOM homes the cursor either way, and DECCOLM clears the screen.
    support::with_corpus(|t| {
        feed(t, b"\x1b[5;10r\x1b[?6s\x1b[?6h\x1b[3;3H");
        feed(t, b"\x1b[?6r");
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO), "?6 restored homes");
    });
    support::with_corpus(|t| {
        feed(t, b"\x1b[?40h\x1b[?3s\x1b[?3htext\x1b[?3r");
        assert!(
            screen_text(t).iter().all(|l| l.is_empty()),
            "?3 restored clears, as DECCOLM does"
        );
    });
}

/// **A restore changes only a mode whose value differs from the one
/// saved**, so restoring a whole tracking family puts back the level that
/// was saved, in either order -- a reset of `?1003` restored while `?1002`
/// is in force is no change to make. A mode restored to a value it already
/// has keeps every effect it has: `?1049 r` saved set, on the alternate
/// screen, does not clear it. The text is silent on a value already in
/// force; xterm re-applies `?6` whatever its value, and `?3` too under its
/// `c132` resource, and baud's no change was decided 2026-10-10 by Jev at
/// 0.96 and 0.94 (XT-7), listed for the owner's veto.
#[test]
fn a_restore_changes_only_what_differs_from_the_saved_value() {
    for restore in [&b"\x1b[?1000;1002;1003r"[..], b"\x1b[?1003;1002;1000r"] {
        support::with_corpus(|t| {
            feed(t, b"\x1b[?1002h\x1b[?1000;1002;1003s\x1b[?1003h");
            feed(t, restore);
            assert_eq!(t.mouse_tracking(), MouseTracking::ButtonEvent);
            assert_eq!(ask(t, 1002), 1);
        });
    }
    for restore in [&b"\x1b[?1006;1015r"[..], b"\x1b[?1015;1006r"] {
        support::with_corpus(|t| {
            feed(t, b"\x1b[?1006h\x1b[?1006;1015s\x1b[?1015h");
            feed(t, restore);
            assert_eq!(ask(t, 1006), 1, "the encoding saved is back");
            assert_eq!(ask(t, 1015), 2);
        });
    }
    support::with_corpus(|t| {
        // `?1002` saved reset while `?1000` was in force, and `?1000` in
        // force again at the restore: `?1002` is reset already.
        feed(
            t,
            b"\x1b[?1000h\x1b[?1002s\x1b[?1002h\x1b[?1000h\x1b[?1002r",
        );
        assert_eq!(t.mouse_tracking(), MouseTracking::Normal);
        // While `?1002` is in force, restoring it reset turns tracking off:
        // DECRST of any level does. xterm restores the level saved under it,
        // `?1000` -- the owner's call.
        feed(t, b"\x1b[?1002h\x1b[?1002r");
        assert_eq!(t.mouse_tracking(), MouseTracking::Off);
    });
    support::with_corpus(|t| {
        feed(t, b"\x1b[?1049h\x1b[2;2Hkept\x1b[?1049s\x1b[?1049r");
        assert_eq!(
            text_of(t.row(Row::from_index(1)).expect("a row on the grid")),
            " kept",
            "not cleared"
        );
    });
}

/// **A value already in force is not set again, at the modes xterm sets
/// again.** xterm's `restoremodes` homes on `?6` whatever the saved value,
/// and on `?3`, under its `c132` resource only, clears and homes
/// (`[[source: xterm-reset]]`); baud changes nothing (XT-7).
#[test]
fn a_restore_at_the_value_in_force_neither_homes_nor_clears() {
    support::with_corpus(|t| {
        feed(t, b"\x1b[?6h\x1b[?6s\x1b[3;4H\x1b[?6r");
        assert_eq!(
            t.cursor(),
            (Row::from_index(2), Col::at_index(3)),
            "?6 did not home"
        );
    });
    support::with_corpus(|t| {
        feed(t, b"\x1b[?3h\x1b[?3s\x1b[2;2Hkept\x1b[?3r");
        assert_eq!(
            text_of(t.row(Row::from_index(1)).expect("a row on the grid")),
            " kept",
            "?3 did not clear"
        );
    });
}

/// **Only what was saved is restored, and only what is listed.** A mode
/// never saved has no value "previously saved", so a restore of it changes
/// nothing -- xterm restores it from a zeroed array, which turns `?25` off and
/// `?7` off; the owner's call.
#[test]
fn only_what_was_saved_and_listed_is_restored() {
    support::with_corpus(|t| {
        feed(t, b"\x1b[?7s\x1b[?7l\x1b[?25l\x1b[?7;25r");
        assert_eq!(ask(t, 7), 1, "?7 was saved and is back");
        assert!(!t.cursor_visible(), "?25 was never saved and stays hidden");
    });
    support::with_corpus(|t| {
        feed(t, b"\x1b[?7;25s\x1b[?7l\x1b[?25l\x1b[?7r");
        assert_eq!(ask(t, 7), 1, "listed, restored");
        assert!(!t.cursor_visible(), "saved and not listed, not restored");
        feed(t, b"\x1b[?25r");
        assert!(t.cursor_visible(), "listed now");
    });
    support::with_corpus(|t| {
        // `?2` never saved: restoring it must not drop the terminal into
        // VT52, which DECRST `?2` would.
        feed(t, b"\x1b[?2r");
        assert_eq!(t.terminal_mode(), TerminalMode::Ansi);
        feed(t, b"\x1b[?2s\x1b[?2r");
        assert_eq!(
            t.terminal_mode(),
            TerminalMode::Ansi,
            "saved ANSI, still ANSI"
        );
    });
}

/// **One level, as DECSC's is**: a second save replaces the first, and a
/// restore leaves the saved value for the next restore, as DECRC leaves
/// DECSC's cursor.
#[test]
fn the_cache_is_one_level_and_a_restore_keeps_it() {
    support::with_corpus(|t| {
        feed(t, b"\x1b[?7s\x1b[?7l\x1b[?7s\x1b[?7h\x1b[?7r");
        assert_eq!(ask(t, 7), 2, "the second save replaced the first");
        feed(t, b"\x1b[?7h\x1b[?7r");
        assert_eq!(ask(t, 7), 2, "and the restore kept it");
    });
}

/// **A mode DECSET does not act on is counted, on a save and on a restore**,
/// as DECSET counts it -- one a parameter, and the rest of the list still
/// acts. The read-only `?1020` to `?1023` and the permanent `?2027` are
/// among them: DECSET changes none of them.
#[test]
fn a_mode_decset_does_not_act_on_is_counted_by_both() {
    for (bytes, n, what) in [
        (&b"\x1b[?9999s"[..], 1usize, "an unknown mode saved"),
        (b"\x1b[?9999r", 1, "an unknown mode restored"),
        (b"\x1b[?1020s", 1, "the read-only ?1020 saved"),
        (b"\x1b[?2027r", 1, "the permanent ?2027 restored"),
        (b"\x1b[?1021s", 1, "the read-only ?1021 saved"),
        (b"\x1b[?1023r", 1, "the read-only ?1023 restored"),
        (b"\x1b[?9999;1020;7s", 2, "two of three"),
        (b"\x1b[?7;25s", 0, "known modes"),
        (b"\x1b[?7;25r", 0, "known modes restored, saved or not"),
    ] {
        support::with_corpus(|t| {
            let (u, p) = (t.unsupported, t.unsupported_param);
            feed(t, bytes);
            assert_eq!(t.unsupported, u, "{what}: not an unsupported sequence");
            assert_eq!(t.unsupported_param, p + n, "{what}");
        });
    }
    support::with_corpus(|t| {
        feed(t, b"\x1b[?9999;7s\x1b[?7l\x1b[?7r");
        assert_eq!(ask(t, 7), 1, "the known mode beside an unknown one acted");
    });
}

/// **The `?` keeps them apart from `CSI s` and `CSI r`.** XTSAVE is not
/// SCOSC and saves no cursor; XTRESTORE is not DECSTBM and sets no region nor
/// homes the cursor; under DECLRMM XTSAVE is not DECSLRM. Each also acts, so
/// a parse that dropped them is caught as well as one that misread them.
#[test]
fn the_question_mark_keeps_them_apart_from_scosc_decstbm_and_decslrm() {
    support::with_corpus(|t| {
        feed(t, b"\x1b[5;5H\x1b[?25s\x1b[1;1H\x1b[?25l\x1b[u\x1b[?25r");
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO), "no cursor was saved");
        assert!(t.cursor_visible(), "the mode was");
        // The control: SCOSC itself.
        feed(t, b"\x1b[5;5H\x1b[s\x1b[1;1H\x1b[u");
        assert_eq!(t.cursor(), (Row::from_index(4), Col::at_index(4)));
    });
    support::with_corpus(|t| {
        feed(t, b"\x1b[?7s\x1b[?7l\x1b[5;5H\x1b[?2;7r");
        assert_eq!(
            t.cursor(),
            (Row::from_index(4), Col::at_index(4)),
            "not homed"
        );
        assert_eq!(
            t.scroll_region(),
            (Row::ZERO, Row::from_index(49)),
            "no region"
        );
        assert_eq!(ask(t, 7), 1, "the mode came back");
        // The control: DECSTBM itself.
        feed(t, b"\x1b[2;7r");
        assert_eq!(t.scroll_region(), (Row::from_index(1), Row::from_index(6)));
    });
    support::with_corpus(|t| {
        feed(t, b"\x1b[?69h\x1b[5;5H\x1b[?1;25s");
        assert_eq!(
            t.cursor(),
            (Row::from_index(4), Col::at_index(4)),
            "not homed"
        );
        feed(t, b"\x1b[?25l\x1b[?25r");
        assert!(t.cursor_visible(), "the mode was saved");
        // The control: DECSLRM homes.
        feed(t, b"\x1b[1;2s");
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO));
    });
}

/// **`RIS` empties the cache and `DECSTR` keeps it.** ECMA-48 § 8.3.105:
/// "RIS causes a device to be reset to its initial state, i.e. the state it
/// has after it is made operational" (`[[source: ecma48]]`), and at power-on
/// nothing is saved. DECSTR's list does not name it. xterm keeps it across
/// both -- `ReallyReset` never touches `save_modes` -- which departs from the
/// text; baud follows the text, decided 2026-10-10 by Jev at 0.94 and 0.94
/// (XT-8), listed for the owner's veto.
#[test]
fn ris_empties_the_cache_and_decstr_keeps_it() {
    support::with_corpus(|t| {
        feed(t, b"\x1b[?7s\x1b[!p\x1b[?7l\x1b[?7r");
        assert_eq!(ask(t, 7), 1, "DECSTR kept what was saved");
    });
    support::with_corpus(|t| {
        feed(t, b"\x1b[?7s\x1bc\x1b[?7l\x1b[?7r");
        assert_eq!(ask(t, 7), 2, "RIS left nothing to restore");
    });
}

/// **Where the reads fall does not change what is saved or restored**,
/// with every sequence split at every byte. Autowrap on a twenty-column
/// screen puts the modes on the screen itself.
#[test]
fn the_save_and_restore_do_not_depend_on_where_the_reads_fell() {
    let ws = WinSize {
        ws_col: 20,
        ws_row: 6,
    };
    let prog: &[u8] = b"\x1b[?7;25;1000s\x1b[?7l\x1b[?25l\x1b[?1002h\
        a line far longer than twenty columns\r\n\
        \x1b[?7;1000r and this one wraps again past twenty\r\n\
        \x1b[?25r\x1b[?7l\x1b[?7s\x1b[?7h\x1b[?7r clipped once more past the edge";
    let run = |n: usize| {
        let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::NONE);
        let mut t = Term::new(owned.storage(), &ws);
        for c in prog.chunks(n) {
            support::feed(&mut t, c);
        }
        (
            screen_text(&t),
            t.cursor(),
            t.cursor_visible(),
            t.mouse_tracking(),
            ask(&mut t, 7),
        )
    };
    let whole = run(4096);
    assert!(whole.2, "the control: ?25 restored");
    // `?1000` was saved reset and is reset again under `?1002`, so its
    // restore had nothing to change.
    assert_eq!(
        whole.3,
        MouseTracking::ButtonEvent,
        "the control: ?1002 kept"
    );
    assert_eq!(whole.4, 2, "the control: the second save of ?7 restored");
    for n in 1..=8 {
        assert_eq!(run(n), whole, "read size {n}");
    }
}

/// **Every mode DECSET changes is saved and restored, and no other is.**
/// The whole range DECSET can name in practice, each against the core's
/// own behaviour rather than a list: a mode whose `DECRQM` answer a DECSET or
/// DECRST moves comes back on restore, and a mode that does not move --
/// unknown, permanent, or read-only -- is counted by both. `?2` is the one
/// exception the harness cannot drive: its reset enters VT52, where no
/// `CSI` follows, so it is held to being saved without a count.
#[test]
fn every_mode_decset_changes_is_saved_and_restored_and_no_other_is() {
    let ws = WinSize {
        ws_col: 20,
        ws_row: 6,
    };
    let (mut acted, mut refused) = (0usize, 0usize);
    for mode in (0u16..=3000).chain([9999]) {
        let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::NONE);
        let mut t = Term::new(owned.storage(), &ws);
        // `?3` moves only while `?40` allows it (`column_mode.rs`).
        if mode == 3 {
            feed_str(&mut t, "\x1b[?40h");
        }
        let was = ask(&mut t, mode);
        let p = t.unsupported_param;
        if mode == 2 {
            feed_str(&mut t, "\x1b[?2s\x1b[?2r");
            assert_eq!(t.unsupported_param, p, "?2 is saved and restored");
            continue;
        }
        let moves = matches!(was, 1 | 2) && {
            flip(&mut t, mode);
            let now = ask(&mut t, mode);
            flip(&mut t, mode);
            now != was
        };
        let p = t.unsupported_param;
        feed_str(&mut t, &format!("\x1b[?{mode}s"));
        match moves {
            true => {
                assert_eq!(t.unsupported_param, p, "?{mode}: saved, not counted");
                flip(&mut t, mode);
                feed_str(&mut t, &format!("\x1b[?{mode}r"));
                assert_eq!(ask(&mut t, mode), was, "?{mode}: restored");
                acted += 1;
            }
            false => {
                assert_eq!(t.unsupported_param, p + 1, "?{mode}: a save counted");
                feed_str(&mut t, &format!("\x1b[?{mode}r"));
                assert_eq!(t.unsupported_param, p + 2, "?{mode}: a restore counted");
                refused += 1;
            }
        }
    }
    assert!(acted > 30, "the control: {acted} modes moved under DECSET");
    assert!(refused > 2900, "the control: {refused} modes were counted");
}

/// **`?47` and `?1049` share one state, so a restore of both is in the
/// order listed.** Saved on the primary, both read reset; restored from the
/// alternate screen `?1049` entered, the first listed leaves and the second
/// then finds itself reset already. Listed `?1049` first, the cursor it
/// saved comes back; listed `?47` first, the bare switch leaves it behind.
/// xterm restores both as the bare switch.
#[test]
fn the_two_alternate_screen_modes_restore_in_the_order_listed() {
    for (restore, cursor, what) in [
        (&b"\x1b[?1049;47r"[..], 4, "?1049 first restores the cursor"),
        (b"\x1b[?47;1049r", 9, "?47 first leaves it where it was"),
    ] {
        support::with_corpus(|t| {
            feed(t, b"\x1b[5;5H\x1b[?47;1049s\x1b[?1049h\x1b[10;10H");
            feed(t, restore);
            assert_eq!(t.screen(), Screen::Normal, "{what}: left");
            assert_eq!(
                t.cursor(),
                (Row::from_index(cursor), Col::at_index(cursor)),
                "{what}"
            );
        });
    }
}
