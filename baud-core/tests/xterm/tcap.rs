//! XTGETTCAP, `DCS + q Pt ST`, and XTSETTCAP, `DCS + p Pt ST`
//! (`[[source: ctlseqs]]` 535-582).
//!
//! A name is hex, `;` between names; the answer is `DCS 1 + r` and each
//! `name=value` pair in one response, the value in hex, or `DCS 0 + r ST`
//! when the first name is not in xterm's table. A key's value is what the
//! key would send now. These tests feed bytes and read the reply alone, so
//! the ones that do not tell the core a `TERM` read the same against a
//! core that answered nothing.

use crate::support;
use baud_core::*;

/// Hex, upper case, as xterm writes a value.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// `DCS + q` asking for `names`, hex-encoded and joined with `;`.
fn ask(names: &[&str]) -> Vec<u8> {
    let list: Vec<String> = names.iter().map(|n| hex(n.as_bytes())).collect();
    format!("\x1bP+q{}\x1b\\", list.join(";")).into_bytes()
}

/// The answer xterm's shape gives `pairs`, in seven-bit controls.
fn valid(pairs: &[(&str, &[u8])]) -> Vec<u8> {
    let list: Vec<String> = pairs
        .iter()
        .map(|(n, v)| format!("{}={}", hex(n.as_bytes()), hex(v)))
        .collect();
    format!("\x1bP1+r{}\x1b\\", list.join(";")).into_bytes()
}

const REFUSED: &[u8] = b"\x1bP0+r\x1b\\";

fn reply(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    support::feed(t, bytes);
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

/// **A key answers what it sends in the modes in force**: an arrow is
/// `CSI A` with DECCKM reset and `SS3 A` set, as the key itself is.
#[test]
fn xtgettcap_answers_a_key_as_it_would_be_sent_now() {
    support::with_corpus(|t| {
        assert_eq!(reply(t, &ask(&["kcuu1"])), valid(&[("kcuu1", b"\x1b[A")]));
        support::feed(t, b"\x1b[?1h");
        assert_eq!(reply(t, &ask(&["kcuu1"])), valid(&[("kcuu1", b"\x1bOA")]));
        // The termcap code is the same row.
        assert_eq!(reply(t, &ask(&["ku"])), valid(&[("ku", b"\x1bOA")]));
        // Backspace is DEL until DECBKM makes it BS.
        assert_eq!(reply(t, &ask(&["kbs"])), valid(&[("kbs", b"\x7f")]));
        support::feed(t, b"\x1b[?67h");
        assert_eq!(reply(t, &ask(&["kbs"])), valid(&[("kbs", b"\x08")]));
        // `kf13` is F13, the VT220's `CSI 25 ~`, as terminfo(5) and xterm's
        // table name it, and not ncurses' shift-F1.
        assert_eq!(reply(t, &ask(&["kf13"])), valid(&[("kf13", b"\x1b[25~")]));
        // A row's modifiers: `kLFT` is shifted Left, `kDN5` control-Down.
        assert_eq!(reply(t, &ask(&["kLFT"])), valid(&[("kLFT", b"\x1b[1;2D")]));
        assert_eq!(reply(t, &ask(&["kDN5"])), valid(&[("kDN5", b"\x1b[1;5B")]));
        // The keypad follows DECKPAM: `kp1` types `1`, then `SS3 q`.
        assert_eq!(reply(t, &ask(&["kp1"])), valid(&[("kp1", b"1")]));
        support::feed(t, b"\x1b=");
        assert_eq!(reply(t, &ask(&["kp1"])), valid(&[("kp1", b"\x1bOq")]));
    });
}

/// **`kcbt` is what Shift-Tab sends**: `CSI Z`, and at `modifyOtherKeys`
/// level 2 `CSI 27 ; 2 ; 9 ~`, as ctlseqs' description of the level gives
/// it (Jev, LIVE, 0.98/0.94).
#[test]
fn xtgettcap_back_tab_follows_modify_other_keys() {
    support::with_corpus(|t| {
        assert_eq!(reply(t, &ask(&["kcbt"])), valid(&[("kcbt", b"\x1b[Z")]));
        support::feed(t, b"\x1b[>4;1m");
        assert_eq!(reply(t, &ask(&["kcbt"])), valid(&[("kcbt", b"\x1b[Z")]));
        support::feed(t, b"\x1b[>4;2m");
        assert_eq!(
            reply(t, &ask(&["kcbt"])),
            valid(&[("kcbt", b"\x1b[27;2;9~")])
        );
    });
}

/// **A key that sends nothing answers the empty string**, and the list
/// goes on: one baud has no code for (Help), F21 before a kitty flag, and
/// every key while KAM locks the keyboard.
#[test]
fn xtgettcap_a_key_that_sends_nothing_answers_empty() {
    support::with_corpus(|t| {
        assert_eq!(
            reply(t, &ask(&["khlp", "kcuu1"])),
            valid(&[("khlp", b""), ("kcuu1", b"\x1b[A")])
        );
        assert_eq!(reply(t, &ask(&["kf21"])), valid(&[("kf21", b"")]));
        support::feed(t, b"\x1b[>1u");
        assert_eq!(
            reply(t, &ask(&["kf21"])),
            valid(&[("kf21", b"\x1b[57384u")])
        );
        support::feed(t, b"\x1b[<u\x1b[2h");
        assert_eq!(reply(t, &ask(&["kcuu1"])), valid(&[("kcuu1", b"")]));
        support::feed(t, b"\x1b[2l");
        assert_eq!(reply(t, &ask(&["kcuu1"])), valid(&[("kcuu1", b"\x1b[A")]));
    });
}

/// **`Co`, `colors` and `RGB`**: 256 colours and 8 bits a channel.
#[test]
fn xtgettcap_answers_the_colour_names() {
    support::with_corpus(|t| {
        assert_eq!(
            reply(t, &ask(&["Co", "colors", "RGB"])),
            valid(&[("Co", b"256"), ("colors", b"256"), ("RGB", b"8")])
        );
    });
}

/// **A name not in xterm's table is refused**, `DCS 0 + r ST`, and so is a
/// list that does not spell one: empty, an odd number of digits, a byte
/// that is no hex digit. Output strings and the extensions programs ask
/// for are outside the interface, "intentionally not part of" it.
#[test]
fn xtgettcap_refuses_a_name_not_in_xterms_table() {
    support::with_corpus(|t| {
        for q in [
            ask(&["cup"]),
            ask(&["Ms"]),
            ask(&["Tc"]),
            ask(&["setrgbf"]),
            ask(&["KCUU1"]),
            b"\x1bP+q\x1b\\".to_vec(),
            b"\x1bP+q;\x1b\\".to_vec(),
            b"\x1bP+q6B637575\x1b\\".to_vec(),
            b"\x1bP+q6B6375753\x1b\\".to_vec(),
            b"\x1bP+q6B63757531X\x1b\\".to_vec(),
        ] {
            assert_eq!(reply(t, &q), REFUSED, "{q:?}");
        }
    });
}

/// **neovim's truecolor query is refused at its first name**, and that is
/// the answer the text gives. Without `COLORTERM`, as over ssh, neovim asks
/// `Tc;RGB;setrgbf;setrgbb` in one DCS (`[[source: neovim-core-defaults-lua]]`
/// 1013-1027), each name in upper-case hex (`[[source: neovim-text-lua]]`
/// 80-104, `[[source: neovim-tty-lua]]` 109-113). `Tc` is not in xterm's
/// table, so the list ends there with `DCS 0 + r ST`, `RGB` unanswered;
/// neovim finds truecolor from its DECRQSS probe instead
/// (`neovims_truecolor_probe_is_answered_in_t416s_form`).
#[test]
fn neovims_truecolor_query_is_refused_at_tc() {
    support::with_corpus(|t| {
        let q = b"\x1bP+q5463;524742;73657472676266;73657472676262\x1b\\";
        assert_eq!(q.to_vec(), ask(&["Tc", "RGB", "setrgbf", "setrgbb"]));
        assert_eq!(reply(t, q), REFUSED);
        // `RGB` alone answers, so the refusal is `Tc`'s.
        assert_eq!(reply(t, &ask(&["RGB"])), valid(&[("RGB", b"8")]));
    });
}

/// **One response, ending at the first name refused** (Jev, R1,
/// 0.90/0.92): what came before it is answered, and nothing after it.
#[test]
fn xtgettcap_answers_a_list_in_one_response_up_to_a_refused_name() {
    support::with_corpus(|t| {
        assert_eq!(
            reply(t, &ask(&["kcuu1", "kcud1"])),
            valid(&[("kcuu1", b"\x1b[A"), ("kcud1", b"\x1b[B")])
        );
        assert_eq!(
            reply(t, &ask(&["kcuu1", "cup", "kcud1"])),
            valid(&[("kcuu1", b"\x1b[A")])
        );
        assert_eq!(
            reply(t, b"\x1bP+q6B63757531;\x1b\\"),
            valid(&[("kcuu1", b"\x1b[A")])
        );
        assert_eq!(reply(t, &ask(&["cup", "kcuu1"])), REFUSED);
    });
}

/// **A name in lower-case hex is found, and echoed as it was sent**, as
/// vim sends it (`%02x`); the value is upper case.
#[test]
fn xtgettcap_reads_either_case_and_echoes_the_name_as_sent() {
    support::with_corpus(|t| {
        assert_eq!(
            reply(t, b"\x1bP+q6b63757531\x1b\\"),
            b"\x1bP1+r6b63757531=1B5B41\x1b\\"
        );
    });
}

/// **`S8C1T` moves the introducer, the terminator and the key's own
/// `CSI`**, every one of them eight-bit.
#[test]
fn xtgettcap_answers_in_eight_bit_controls_after_s8c1t() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b G");
        assert_eq!(reply(t, &ask(&["kcuu1"])), b"\x90\x31+r6B63757531=9B41\x9c");
        assert_eq!(reply(t, &ask(&["cup"])), b"\x900+r\x9c");
    });
}

/// **The request reads the same whatever the read boundaries**: fed a byte
/// at a time, the answer is the one a single read gives.
#[test]
fn xtgettcap_reads_the_same_split_at_every_byte() {
    let q = ask(&["kcuu1", "Co", "khlp"]);
    let whole = support::with_corpus(|t| reply(t, &q));
    let split = support::with_corpus(|t| {
        for b in &q {
            support::feed(t, std::slice::from_ref(b));
        }
        t.reply().to_vec()
    });
    assert_eq!(whole, split);
    assert_eq!(
        whole,
        valid(&[("kcuu1", b"\x1b[A"), ("Co", b"256"), ("khlp", b"")])
    );
}

/// **After a parameter, `DCS +` is no XTGETTCAP**: xterm's `do_dcs`
/// switches on the string's first byte. It is consumed and counted.
#[test]
fn xtgettcap_after_a_parameter_is_another_dcs() {
    support::with_corpus(|t| {
        let ignored = t.strings_ignored;
        assert_eq!(reply(t, b"\x1bP1+q6B63757531\x1b\\"), b"");
        assert_eq!(t.strings_ignored, ignored + 1);
    });
}

/// **A list cut at the string buffer's end stops before the cut name**,
/// which an even-length cut could make a different name, and the cut is
/// counted.
#[test]
fn xtgettcap_a_cut_list_ends_before_the_name_the_cut_fell_in() {
    support::with_corpus(|t| {
        // 102 names of four digits and `;` are 509 bytes; the 512-byte
        // buffer, its `q` first, keeps the `;` after them and the next
        // name's first digit, and cuts the rest.
        let mut names = vec!["kb"; 103];
        names[102] = "kcuu1";
        let truncations = t.string_truncations;
        let got = reply(t, &ask(&names));
        let want: Vec<(&str, &[u8])> = vec![("kb", b"\x7f"); 102];
        assert_eq!(got, valid(&want));
        assert!(t.string_truncations > truncations);
        // The same list one name shorter is not cut, and is answered whole.
        let got = reply(t, &ask(&names[..102]));
        assert_eq!(got, valid(&want));
        // **A cut that leaves a name of its own is still refused**: 101
        // names and a `;` are 505 bytes, so the buffer keeps six digits of
        // `kf10`, which spell `kf1`.
        let mut names = vec!["kb"; 102];
        names[101] = "kf10";
        let got = reply(t, &ask(&names));
        assert_eq!(got, valid(&want[..101]));
    });
}

/// **A full list's answer fits the reply buffer alone, and behind other
/// replies it is sent whole or dropped and counted**, never cut.
#[test]
fn xtgettcap_is_sent_whole_or_dropped_and_counted() {
    support::with_corpus(|t| {
        let q = ask(&vec!["kb"; 102]);
        let alone = reply(t, &q);
        assert!(alone.starts_with(b"\x1bP1+r") && alone.ends_with(b"\x1b\\"));
        // Fill the buffer with DA1 answers until one more would not fit.
        let room = REPLY - alone.len() + 1;
        let da = reply(t, b"\x1b[c");
        let mut pending = Vec::new();
        while pending.len() < room {
            support::feed(t, b"\x1b[c");
            pending.extend_from_slice(&da);
        }
        let before = t.reply_overflows;
        support::feed(t, &q);
        assert_eq!(t.reply(), &pending[..], "nothing of it is sent");
        assert_eq!(t.reply_overflows, before + 1);
    });
}

/// **`TN` and `name` answer the `TERM` the embedder named**; untold, they
/// are refused, and `RIS` keeps what it was told.
#[test]
fn xtgettcap_names_the_terminal_description_the_embedder_exports() {
    support::with_corpus(|t| {
        assert_eq!(reply(t, &ask(&["TN"])), REFUSED);
        t.set_term_name(TermName::new(b"xterm-256color").expect("a name"));
        assert_eq!(
            reply(t, &ask(&["TN", "name"])),
            valid(&[("TN", b"xterm-256color"), ("name", b"xterm-256color")])
        );
        support::feed(t, b"\x1bc");
        assert_eq!(reply(t, &ask(&["TN"])), valid(&[("TN", b"xterm-256color")]));
    });
    assert!(TermName::new(b"").is_none());
    assert!(TermName::new(b"xterm 256color").is_none());
    assert!(TermName::new(&[b'x'; 33]).is_none());
    assert!(TermName::new(&[b'x'; 32]).is_some());
}

/// **XTSETTCAP is refused and counted**: baud has no terminal database to
/// retrieve an entry from, so it changes nothing, `TN` included.
#[test]
fn xtsettcap_is_refused_and_counted() {
    support::with_corpus(|t| {
        t.set_term_name(TermName::new(b"xterm-256color").expect("a name"));
        let unsupported = t.unsupported;
        let q = format!("\x1bP+p{}\x1b\\", hex(b"vt100"));
        assert_eq!(t.tcap_sets_refused(), TcapSetsRefused::NONE);
        assert_eq!(reply(t, q.as_bytes()), b"");
        let once = t.tcap_sets_refused();
        assert_ne!(once, TcapSetsRefused::NONE);
        assert_eq!(t.unsupported, unsupported + 1);
        support::feed(t, q.as_bytes());
        assert_ne!(t.tcap_sets_refused(), once, "each is counted");
        assert_eq!(reply(t, &ask(&["TN"])), valid(&[("TN", b"xterm-256color")]));
    });
}
