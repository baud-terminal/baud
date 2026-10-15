//! `CSI s` and `CSI u` with parameters, while DECLRMM is reset.
//!
//! **SCOSC and SCORC take no parameters.** `[[source: ctlseqs]]` 1592-1593
//! gives "CSI s  Save cursor, available only when DECLRMM is disabled (SCOSC,
//! also ANSI.SYS)" and 1734 "CSI u  Restore cursor (SCORC, also ANSI.SYS)";
//! the parametered `s` is another function, 1595-1597, "CSI Pl ; Pr s  Set
//! left and right margins (DECSLRM) ... available only when DECLRMM is
//! enabled", which `[[source: dec-std-070]]` DECSLRM note 4 (p. 5-27) puts
//! as "only recognized when DECLRMM ... is set". The pair's own texts carry
//! no parameter either: SCP is `ESC [ s` and RCP `ESC [ u` in
//! `[[source: msdos2-ansi-doc]]` (84, 91), in `[[source: mslearn-ansi-sys]]`
//! (794-797) and in `[[source: sco-osr507-screen-hw]]` (2595, 2623).
//!
//! **A defaulted parameter is no parameter.** The same ANSI.DOC, note 1
//! (6-7): "The default value is used when no explicit value is given or a
//! value of zero is given"; STD 070 § 3.5.3.1 (p. 3-24): "A zero length
//! parameter, or one consisting only of 3/0 bit combinations, represents a
//! default whose value depends upon the control function". So `CSI 0 s`,
//! `CSI ; s` and `CSI 0;0 s` are SCOSC written with its defaults, as
//! `CSI 0;0 s` under `?69h` is DECSLRM written with its. xterm saves only on
//! a bare `CSI s` or one empty parameter (`only_default`, charproc.c 2220
//! and 4899, `[[source: xterm-reset]]`), so `CSI 0 s` saves nothing there:
//! xterm's departure.
//!
//! **A parameter that is not a default is DECSLRM's shape**, and DECSLRM is
//! not available, so the sequence is ignored and counted in
//! `Term::unsupported` -- STD 070 § 3.5.1.3 (p. 3-20), "Unimplemented
//! functions shall be ignored as if they were not received". `CSI 5 u`
//! matches no form at all. Until 2026-10-14 every one of them saved or
//! restored the cursor. Reading § 3.5.1.3's other sentence -- an
//! unimplemented parameter "ignored as if that parameter were not received"
//! -- as making `CSI 5 s` a SCOSC and `CSI 5 u` a SCORC is the other
//! option, open in the record.
//!
//! Each case saves with `ESC 7` first, so a sequence that saves or restores
//! nothing shows as `ESC 8` or the cursor finding that first save, and none
//! of them leans on what a restore with nothing saved does (note AE's Q13).
//! Held at every read size and every split into two reads.

use crate::support;
use baud_core::*;

/// The cursor and the unsupported count after `bytes`, fed in reads that
/// end at each of `cuts`.
fn fed(bytes: &[u8], cuts: &[usize]) -> ((usize, usize), usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let mut from = 0;
    for &to in cuts.iter().chain([bytes.len()].iter()) {
        support::feed(&mut t, &bytes[from..to]);
        from = to;
    }
    let (r, c) = t.cursor();
    ((r.index(), c.index()), t.unsupported)
}

/// `bytes` lands the cursor on `want` and counts `counted` more than
/// `control`, which is `bytes` without the sequence under test, fed whole --
/// whole, at every read size and at every split into two reads.
fn holds(what: &str, bytes: &[u8], control: &[u8], want: (usize, usize), counted: usize) {
    let base = fed(control, &[]).1;
    let check = |at: &str, (cursor, unsupported): ((usize, usize), usize)| {
        assert_eq!(cursor, want, "{what}, {at}: cursor");
        assert_eq!(unsupported, base + counted, "{what}, {at}: unsupported");
    };
    check("whole", fed(bytes, &[]));
    for size in 1..=bytes.len() {
        let cuts: Vec<usize> = (size..bytes.len()).step_by(size).collect();
        check(&format!("read size {size}"), fed(bytes, &cuts));
    }
    for at in 1..bytes.len() {
        check(&format!("split at byte {at}"), fed(bytes, &[at]));
    }
}

/// Every parameter defaulted: omitted, empty or 0, one or two of them.
const DEFAULTED: &[&str] = &["", "0", ";", "0;0", "00", ";0", "0;"];
/// A parameter that is not a default, first, second or third, and a colon,
/// which STD 070 § 3.5.3.1 (p. 3-24) says makes "the entire sequence up to
/// and including the final character" ignored.
const NOT_DEFAULTED: &[&str] = &["5", "1;2", "0;5", "5;0", ";5", "0;0;3", "1;2;3", "0:0"];

/// **SCOSC with every parameter defaulted saves the cursor.** Green before
/// the change too, which saved on every `CSI s`; this half pins that the
/// gate reads 0 and an empty parameter as the default, as the texts do.
#[test]
fn scosc_with_every_parameter_defaulted_saves() {
    for p in DEFAULTED {
        let s = format!("\x1b[2;2H\x1b7\x1b[5;7H\x1b[{p}s\x1b[9;9H\x1b8");
        let control = "\x1b[2;2H\x1b7\x1b[5;7H\x1b[9;9H\x1b8";
        holds(
            &format!("CSI {p} s"),
            s.as_bytes(),
            control.as_bytes(),
            (4, 6),
            0,
        );
    }
}

/// **A `CSI s` with a parameter that is not a default saves nothing and is
/// counted.** `ESC 8` finds the `ESC 7` save at 2;2, not 5;7.
#[test]
fn scosc_with_a_parameter_saves_nothing_and_is_counted() {
    for p in NOT_DEFAULTED {
        let s = format!("\x1b[2;2H\x1b7\x1b[5;7H\x1b[{p}s\x1b[9;9H\x1b8");
        let control = "\x1b[2;2H\x1b7\x1b[5;7H\x1b[9;9H\x1b8";
        holds(
            &format!("CSI {p} s"),
            s.as_bytes(),
            control.as_bytes(),
            (1, 1),
            1,
        );
    }
}

/// **SCORC with every parameter defaulted restores the cursor**, to the
/// `ESC 7` save at 2;2. Green before the change too.
#[test]
fn scorc_with_every_parameter_defaulted_restores() {
    for p in DEFAULTED {
        let u = format!("\x1b[2;2H\x1b7\x1b[5;7H\x1b[{p}u");
        let control = "\x1b[2;2H\x1b7\x1b[5;7H";
        holds(
            &format!("CSI {p} u"),
            u.as_bytes(),
            control.as_bytes(),
            (1, 1),
            0,
        );
    }
}

/// **A `CSI u` with a parameter that is not a default restores nothing and is
/// counted**: the cursor stays at 5;7.
#[test]
fn scorc_with_a_parameter_restores_nothing_and_is_counted() {
    for p in NOT_DEFAULTED {
        let u = format!("\x1b[2;2H\x1b7\x1b[5;7H\x1b[{p}u");
        let control = "\x1b[2;2H\x1b7\x1b[5;7H";
        holds(
            &format!("CSI {p} u"),
            u.as_bytes(),
            control.as_bytes(),
            (4, 6),
            1,
        );
    }
}

/// **Under DECLRMM the parametered `CSI s` is still DECSLRM**: it sets the
/// margins and homes, saves nothing and is not counted, so the gate is
/// DECLRMM's reset state only. Green before the change too.
#[test]
fn under_declrmm_a_parametered_csi_s_is_still_decslrm() {
    let s = b"\x1b[?69h\x1b[2;2H\x1b7\x1b[5;7H\x1b[3;9s\x1b[2Cx\x1b8";
    let control = b"\x1b[?69h\x1b[2;2H\x1b7\x1b[5;7H\x1b[2Cx\x1b8";
    holds("DECSLRM 3;9", s, control, (1, 1), 0);
    // The margins took: `x` after `CUF 2` from the home at column 1 sits at
    // column 3, inside them.
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    support::feed(&mut t, b"\x1b[?69h\x1b[3;9s\x1b[?6h\x1b[1;1Hx");
    let (r, c) = t.cursor();
    assert_eq!((r.index(), c.index()), (0, 3), "origin at the left margin");
}
