//! **The read record against the core's own dispatch** (E1(b)): a corpus
//! drawn from the core rather than from a hand-written alphabet, each case
//! fed to a terminal whose every line of both screens is marked
//! (`record_check`), and each read's record checked as the generated
//! programs check theirs (`read_record_props.rs`): every line a read
//! changed is named, and every line a read moved text onto is one its
//! moves carried text onto ([`ReadRecord::carried_from`]).
//!
//! Two halves:
//!
//! - **The table walked** ([`table_edges`]): every edge out of every node
//!   the parse table reaches, under both encodings and with left and right
//!   margin mode off and on, each fed after bytes that reach its node. With
//!   `diag`, a counter of the edges the steps took shows the walk reaches
//!   every one of them.
//! - **What the edges dispatch**: every `CSI` form the core parses
//!   (`Csi::parse_with`, both margin modes, every marker, intermediate and
//!   final) under parameter classes, every `ESC` final with and without an
//!   intermediate, every `DCS` form and the payloads the core acts on, the
//!   `OSC` numbers whole and cut across reads, kitty's `APC`, VT52 with
//!   `ESC Y`, the C0 row, and C1 under ISO 8859-1 -- each after the setups
//!   that change what they do: a scroll region, side margins, origin mode,
//!   no autowrap, either alternate screen, ISO 8859-1, the conformance
//!   levels, and double-width lines.
#![cfg(feature = "read-record")]
mod record_check;

use baud_core::*;
use baud_domain::{Csi, FinalByte, InputEncoding, MarginMode, Marker, Param, Params, StreamByte};
use record_check::*;

/// One case: its name for a failure, whether reads are held at a page-width
/// request, and its reads in order. Every case has a printer ready, so the
/// media copy forms act and printer controller mode is entered.
struct Case {
    name: String,
    hold: WidthHold,
    reads: Vec<Vec<u8>>,
}

impl Case {
    fn of(name: String, reads: Vec<Vec<u8>>) -> Case {
        Case {
            name,
            hold: WidthHold::Off,
            reads,
        }
    }
}

/// Whether a walk sets left and right margin mode first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Margins {
    Off,
    On,
}

/// **The table walked under `enc`, margins `m`**: each edge's root set up,
/// the bytes to its node -- cut into two reads where a character must be
/// cut -- and then its byte.
fn edge_cases(enc: InputEncoding, m: Margins) -> Vec<(Case, TableEdge)> {
    table_edges(enc)
        .map(|e| {
            let mut reads = Vec::new();
            if m == Margins::On {
                reads.push(b"\x1b[?69h".to_vec());
            }
            if enc == InputEncoding::Latin1 {
                reads.push(b"\x1b%@".to_vec());
            }
            let mut tail = e.path().to_vec();
            tail.push(e.byte().raw_byte());
            let hold = match e.root() {
                WalkRoot::Ground => WidthHold::Off,
                WalkRoot::Vt52 => {
                    reads.push(b"\x1b[?2l".to_vec());
                    WidthHold::Off
                }
                // The request puts the parser in the printer's node itself,
                // so the character that takes a setup back to the ground
                // goes before it.
                WalkRoot::Printer => {
                    if !reads.is_empty() {
                        reads.push(b"x".to_vec());
                    }
                    let mut printing = b"\x1b[5i".to_vec();
                    printing.append(&mut tail);
                    tail = printing;
                    WidthHold::Off
                }
                WalkRoot::Held => {
                    let mut held = b"\x1b[?40h\x1b[?3h".to_vec();
                    held.append(&mut tail);
                    tail = held;
                    WidthHold::On
                }
            };
            // A setup ends on a control, which leaves the parser after it:
            // a character takes it back to the ground the walk starts from.
            // Then a byte a read, so no fast path takes a sequence whole and
            // every byte is the table's step; a held read is the request's.
            match hold {
                WidthHold::On => reads.push(tail),
                WidthHold::Off => {
                    if !reads.is_empty() && e.root() != WalkRoot::Printer {
                        reads.push(b"x".to_vec());
                    }
                    reads.extend(tail.iter().map(|b| vec![*b]));
                }
            }
            let name = format!(
                "{enc:?} {m:?} {:?} {:?} {:#04x}",
                e.root(),
                e.node(),
                e.byte().raw_byte()
            );
            (Case { name, hold, reads }, e)
        })
        .collect()
}

/// **The setups**: the states that change what a control does to the
/// lines, each leaving the cursor off the top and bottom rows.
const SETUPS: [(&str, &[u8]); 11] = [
    ("power-on", b"\x1b[5;4H"),
    ("region", b"\x1b[2;6r\x1b[4;3H"),
    ("side margins", b"\x1b[?69h\x1b[2;9s\x1b[4;3H"),
    ("origin", b"\x1b[2;6r\x1b[?6h\x1b[2;2H"),
    ("no autowrap", b"\x1b[?7l\x1b[5;10H"),
    ("?1049", b"\x1b[?1049h\x1b[5;4H"),
    ("?47", b"\x1b[?47h\x1b[5;4H"),
    ("latin-1", b"\x1b%@\x1b[5;4H"),
    ("level 1", b"\x1b[61\"p\x1b[5;4H"),
    ("level 4", b"\x1b[64\"p\x1b[5;4H"),
    ("double width", b"\x1b[3;1H\x1b#6\x1b[7;1H\x1b#3\x1b[5;4H"),
];

/// Each form under each setup, the setup its own read.
fn under_setups(forms: Vec<(String, Vec<Vec<u8>>)>) -> Vec<Case> {
    let mut out = Vec::new();
    for (name, reads) in forms {
        for (s, setup) in SETUPS {
            let mut all = vec![setup.to_vec()];
            all.extend(reads.iter().cloned());
            out.push(Case::of(format!("{s}: {name}"), all));
        }
    }
    out
}

/// The parameter classes a `CSI` form is sent with: none, zero, one, a
/// count, a pair, a hole, the screen's edges and one past, `Param`'s
/// ceiling and past it, a sub-parameter, and a list.
const PARAMS: [&str; 13] = [
    "",
    "0",
    "1",
    "3",
    "3;5",
    "1;;4",
    "8;12",
    "9;13",
    "65535",
    "99999",
    "2:3",
    "1;2;3;4;5",
    "4;2;6;8",
];

/// A marker's wire byte.
fn marker_byte(m: Marker) -> &'static str {
    match m {
        Marker::Standard => "",
        Marker::Private => "?",
        Marker::Gt => ">",
        Marker::Lt => "<",
        Marker::Equals => "=",
    }
}

/// **Every `CSI` form the core parses**: each marker, intermediate and
/// final whose parse under either margin mode, bare or with one
/// parameter, is not `Unsupported`, sent with every parameter class.
fn csi_forms() -> Vec<(String, Vec<Vec<u8>>)> {
    let markers = [
        Marker::Standard,
        Marker::Private,
        Marker::Gt,
        Marker::Lt,
        Marker::Equals,
    ];
    let one = [Some(Param::from_raw(1))];
    let mut out = Vec::new();
    for m in markers {
        for inter in core::iter::once(None).chain((0x20..=0x2f).map(Some)) {
            for f in 0x40..=0x7e {
                let fb = FinalByte::from_stream(StreamByte::from_stream(f)).expect("a final");
                let ib = inter.map(StreamByte::from_stream);
                let parsed = [MarginMode::Off, MarginMode::On].into_iter().any(|mode| {
                    [Params::new(&[]), Params::new(&one)].into_iter().any(|p| {
                        !matches!(Csi::parse_with(m, ib, p, fb, mode), Csi::Unsupported { .. })
                    })
                });
                if !parsed {
                    continue;
                }
                let i = inter.map(char::from).map(String::from).unwrap_or_default();
                for p in PARAMS {
                    let wire = format!("\x1b[{}{p}{i}{}", marker_byte(m), char::from(f));
                    out.push((format!("CSI {wire:?}"), vec![wire.into_bytes()]));
                }
            }
        }
    }
    out
}

/// **Every `ESC` final, bare and after each intermediate**: the core's own
/// `esc_final` decides which act, so every pair is sent.
fn esc_forms() -> Vec<(String, Vec<Vec<u8>>)> {
    let mut out = Vec::new();
    for inter in core::iter::once(None).chain((0x20u8..=0x2f).map(Some)) {
        for f in 0x30u8..=0x7e {
            let mut wire = vec![0x1b];
            wire.extend(inter);
            wire.push(f);
            out.push((format!("ESC {wire:?}"), vec![wire]));
        }
    }
    out
}

/// **Every `DCS` form, and the payloads the core acts on**: each
/// intermediate and final with no body, then sixel, ReGIS, DECRSPS's two
/// reports, DECRSTS, XTGETTCAP and XTSETTCAP, DECUDK and DECRQSS with
/// bodies, each with and without a parameter.
fn dcs_forms() -> Vec<(String, Vec<Vec<u8>>)> {
    let mut out = Vec::new();
    for inter in core::iter::once(None).chain((0x20u8..=0x2f).map(Some)) {
        for f in 0x40u8..=0x7e {
            let mut wire = b"\x1bP".to_vec();
            wire.extend(inter);
            wire.push(f);
            wire.extend(b"\x1b\\");
            out.push((format!("DCS {wire:?}"), vec![wire]));
        }
    }
    let bodies: [&[u8]; 12] = [
        b"q#0;2;100;0;0#0~~~~-~~$-!8~",
        b"0;1q\"1;1;6;6#1~~~~~~",
        b"pP[10,10]V[50,50]",
        b"1$t1;1;1;@;@;@;0;2;@;BBBB",
        b"2$t9/17",
        b"1$p\x1b[?1049h\x1b[2;4r",
        b"+q544e",
        b"+p544e=78",
        b"1|1/414243",
        b"$qm",
        b"$qr",
        b"$q\"p",
    ];
    for b in bodies {
        let mut wire = b"\x1bP".to_vec();
        wire.extend(b);
        wire.extend(b"\x1b\\");
        out.push((format!("DCS payload {wire:?}"), vec![wire]));
    }
    out
}

/// **The `OSC` numbers, whole and cut across reads**: each number with a
/// short body ended by `BEL` and by `ST`, and the bodies the core acts on
/// -- links, prompt marks, folders, iTerm2's files, the clipboard -- whole
/// and cut in the middle.
fn osc_forms() -> Vec<(String, Vec<Vec<u8>>)> {
    let mut out = Vec::new();
    let numbers = (0u32..=120).chain([133, 633, 777, 1337, 3008, 5113, 5522]);
    for n in numbers {
        for end in ["\x07", "\x1b\\"] {
            let wire = format!("\x1b]{n};x{end}");
            out.push((format!("OSC {wire:?}"), vec![wire.into_bytes()]));
        }
    }
    let bodies = [
        "8;;file:///etc/hosts",
        "8;;",
        "133;A",
        "133;A;redraw=1",
        "133;B",
        "133;C",
        "133;D;0",
        "133;L",
        "133;N",
        "133;P;k=s",
        "7;file:///tmp",
        "7;file://elsewhere.example/x",
        "1337;CurrentDir=/srv",
        "1337;File=inline=1;size=4:AAAA",
        "1337;MultipartFile=inline=1;size=4",
        "1337;FilePart=AAAA",
        "1337;FileEnd",
        "52;c;aGk=",
        "52;c;?",
    ];
    for b in bodies {
        let wire = format!("\x1b]{b}\x1b\\").into_bytes();
        let cut = wire.len() / 2;
        out.push((format!("OSC {b:?}"), vec![wire.clone()]));
        out.push((
            format!("OSC {b:?} in pieces"),
            vec![wire[..cut].to_vec(), wire[cut..].to_vec()],
        ));
    }
    out
}

/// Kitty's graphics commands in an `APC`: transmit and show, place, query
/// and delete; and a delete left open, as mpv's exit leaves one, with a
/// prompt mark after it, ended by BEL and by ST, whole and cut at every
/// byte into two reads.
fn apc_forms() -> Vec<(String, Vec<Vec<u8>>)> {
    let mut out: Vec<(String, Vec<Vec<u8>>)> = [
        "\x1b_Ga=T,f=24,s=1,v=1;AAAA\x1b\\",
        "\x1b_Ga=t,f=24,s=1,v=1,i=7;AAAA\x1b\\\x1b_Ga=p,i=7\x1b\\",
        "\x1b_Ga=q,i=9,f=24,s=1,v=1;AAAA\x1b\\",
        "\x1b_Ga=d\x1b\\",
    ]
    .into_iter()
    .map(|w| (format!("APC {w:?}"), vec![w.as_bytes().to_vec()]))
    .collect();
    for open in ["\x1b_Ga=d;\x1b]133;D\x07", "\x1b_Ga=d;\x1b]133;D\x1b\\"] {
        let w = open.as_bytes();
        out.push((format!("APC open {open:?}"), vec![w.to_vec()]));
        for cut in 1..w.len() {
            out.push((
                format!("APC open {open:?} cut at {cut}"),
                vec![w[..cut].to_vec(), w[cut..].to_vec()],
            ));
        }
    }
    out
}

/// **VT52**: each final the core acts on, and `ESC Y` to the top, the
/// middle and past the edge, each entered and left inside the case.
fn vt52_forms() -> Vec<(String, Vec<Vec<u8>>)> {
    let mut out = Vec::new();
    for b in 0x20u8..=0x7e {
        let wire = [b"\x1b[?2l\x1b".as_slice(), &[b], b"x\x1b<"].concat();
        out.push((format!("VT52 ESC {:?}", char::from(b)), vec![wire]));
    }
    for (row, col) in [(b' ', b' '), (b'%', b'#'), (b'~', b'~')] {
        let wire = [b"\x1b[?2l\x1bY".as_slice(), &[row, col], b"x\x1b<"].concat();
        out.push((format!("VT52 ESC Y {row} {col}"), vec![wire]));
    }
    out
}

/// The C0 row, each control between text, and C1 under ISO 8859-1.
fn control_forms() -> Vec<(String, Vec<Vec<u8>>)> {
    let mut out = Vec::new();
    for b in 0x00u8..=0x1f {
        out.push((format!("C0 {b:#04x}"), vec![vec![b'a', b, b'b']]));
    }
    for b in 0x80u8..=0x9f {
        let wire = [b"\x1b%@a".as_slice(), &[b], b"2;3Hb\x9c\x1b%G"].concat();
        out.push((format!("C1 {b:#04x}"), vec![wire]));
    }
    out
}

/// **What a mode changes, shown**: after each mode is set or reset, text
/// and line feeds across the bottom (`LNM`, DECAWM, a region's edge), a
/// sixel where its anchor puts it (`?80`), a rendition changed over a
/// rectangle (DECSACE), and a cursor restored from a presentation report
/// (DECCIR).
const PROBE: &[u8] = b"ab\ncd\r\n\x1b[8;11Hxyz\n\x1b[3;2H\x1bPq#0;2;100;0;0#0~~~~-~~\x1b\\\x1b[2;3;4;6;1$r\x1bP1$t2;4;1;@;@;@;0;2;@;BBBB\x1b\\e";

/// **Every mode, set and reset, then shown**: the ANSI modes `SM` and `RM`
/// name, and the private ones the core can know -- DEC's, xterm's mouse and
/// screen modes, and the extensions past them -- each its own read, then
/// [`PROBE`].
fn mode_forms() -> Vec<(String, Vec<Vec<u8>>)> {
    let ansi = (1u32..=30).map(|n| ("", n));
    let private = (1u32..=120)
        .chain(1000..=1070)
        .chain([2004, 2026, 2027, 2031, 7727, 8452])
        .map(|n| ("?", n));
    let mut out = Vec::new();
    for (m, n) in ansi.chain(private) {
        for f in ['h', 'l'] {
            let wire = format!("\x1b[{m}{n}{f}");
            out.push((
                format!("mode {wire:?}"),
                vec![wire.into_bytes(), PROBE.to_vec()],
            ));
        }
    }
    out
}

/// **What a case's reads leave unsaid**: each read fed whole to a marked
/// terminal, its record checked against both screens.
fn unsaid_of(case: &Case) -> Vec<String> {
    with(|t| {
        t.set_width_hold(case.hold);
        t.set_printer(PrinterPort::Ready);
        case.reads
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let (rec, u) = checked(t, r);
                (!u.unnamed.is_empty() || !u.displaced.is_empty())
                    .then(|| format!("{} read {i}: {u:?} in {rec:?}", case.name))
            })
            .collect()
    })
}

/// Every case's failures, the first few in full and the count.
fn report(cases: &[Case]) -> Result<(), String> {
    let failed: Vec<String> = cases.iter().flat_map(unsaid_of).collect();
    match failed.is_empty() {
        true => Ok(()),
        false => Err(format!(
            "{} reads of {} cases leave lines unsaid:\n{}",
            failed.len(),
            cases.len(),
            failed[..failed.len().min(12)].join("\n")
        )),
    }
}

/// **Every edge of the table, fed, leaves nothing unsaid.**
#[test]
fn every_edge_of_the_table_leaves_nothing_unsaid() {
    let mut cases = Vec::new();
    for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
        for m in [Margins::Off, Margins::On] {
            cases.extend(edge_cases(enc, m).into_iter().map(|(c, _)| c));
        }
    }
    assert!(cases.len() > 20_000, "the walk found {} edges", cases.len());
    report(&cases).unwrap();
}

/// **Every form the core dispatches, under every setup, leaves nothing
/// unsaid.**
#[test]
fn every_form_the_core_dispatches_leaves_nothing_unsaid() {
    let mut forms = csi_forms();
    assert!(forms.len() > 1_000, "only {} CSI cases", forms.len());
    forms.extend(esc_forms());
    forms.extend(dcs_forms());
    forms.extend(osc_forms());
    forms.extend(apc_forms());
    forms.extend(control_forms());
    forms.extend(mode_forms());
    let mut cases = under_setups(forms);
    // VT52 under the setups an application in it can have set before.
    cases.extend(under_setups(vt52_forms()));
    report(&cases).unwrap();
}

/// **The walk reaches every edge it names**, under both encodings and
/// both margin modes: the steps' counter of edges taken holds each one.
/// Breaks: feed a walk in one read, where a fast path takes a sequence
/// whole; leave out the character that takes a setup back to the ground;
/// or hold no read at the page-width request.
#[cfg(feature = "diag")]
#[test]
fn the_walk_takes_every_edge_of_the_table() {
    for m in [Margins::Off, Margins::On] {
        let mut took = Hist::ZERO;
        let mut edges = Vec::new();
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for (case, e) in edge_cases(enc, m) {
                with(|t| {
                    t.set_width_hold(case.hold);
                    t.set_printer(PrinterPort::Ready);
                    for r in &case.reads {
                        let _ = checked(t, r);
                    }
                    took.add(&t.hist);
                });
                edges.push((enc, e));
            }
        }
        let missed: Vec<String> = edges
            .iter()
            .filter(|(enc, e)| !took.took(*enc, e.node(), e.byte()))
            .map(|(enc, e)| {
                format!(
                    "{enc:?} {:?} {:?} {:#04x}",
                    e.root(),
                    e.node(),
                    e.byte().raw_byte()
                )
            })
            .collect();
        assert!(
            missed.is_empty(),
            "{m:?}: {} of {} edges not taken: {:?}",
            missed.len(),
            edges.len(),
            &missed[..missed.len().min(20)]
        );
    }
}

/// **The walk reaches every node of the table**, under one encoding or the
/// other: a node only a mode, an embedder's call or a path past the walk's
/// length enters would be outside the corpus, and none is. A node the walk
/// cannot reach goes in `UNWALKED` with its root and the reason, which is
/// empty. Breaks: drop the held root, or the printer's (printer
/// controller mode, which `CSI 5 i` enters from its act, not an edge, and
/// which the first run of this test found the walk missing).
#[test]
fn every_node_is_walked() {
    const UNWALKED: [(Node, &str); 0] = [];
    let walked: Vec<Node> = [InputEncoding::Utf8, InputEncoding::Latin1]
        .into_iter()
        .flat_map(table_edges)
        .map(|e| e.node())
        .collect();
    let missing: Vec<Node> = Node::ALL
        .into_iter()
        .filter(|n| !walked.contains(n) && !UNWALKED.iter().any(|(u, _)| u == n))
        .collect();
    assert!(
        missing.is_empty(),
        "nodes the walk never reaches: {missing:?}"
    );
}
