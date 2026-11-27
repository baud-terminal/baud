//! `ReadSniff`: whether each read of a stream carries a link, in one pass
//! over the carried bytes and the read, against a byte-at-a-time reading of
//! the whole stream and against the two-pass scan it replaces.

use core::ops::Range;

use baud_domain::{
    BytePos, InputEncoding, LinkSniff, ReadLinked, SNIFF_CARRY, ShellTokenHex, StreamByte,
    WalkSteps,
};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

/// Where an `OSC 8` introducer stands, byte by byte over a whole stream: the
/// parser's rows for `ESC`, `OSC` and the string before its `;`, written out
/// by byte class.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum At {
    Ground,
    Esc,
    Osc,
    Eight,
}

impl At {
    /// The step after `b`, and whether `b` ends an introducer. A byte inside
    /// the introducer is read through ISO 8859-1's GR rule, the wider reading.
    fn step(self, b: StreamByte) -> (At, ReadLinked) {
        let raw = b.raw_byte();
        let gl = InputEncoding::Latin1.gr_as_gl(b).raw_byte();
        let next = match (self, raw, gl) {
            (_, 0x1b, _) => At::Esc,
            (_, 0x9d, _) => At::Osc,
            (_, 0x18 | 0x1a, _) => At::Ground,
            (At::Ground, _, _) => At::Ground,
            (At::Esc, _, b']') => At::Osc,
            (At::Esc, _, 0x7f) => At::Esc,
            (At::Esc, _, _) if b.is_c0() => At::Esc,
            (At::Esc, _, _) => At::Ground,
            (At::Osc | At::Eight, 0x07, _) => At::Ground,
            (At::Osc, _, b'8') => At::Eight,
            (At::Eight, _, b';') => return (At::Ground, ReadLinked::Yes),
            (At::Osc | At::Eight, _, _) if b.is_c0() => self,
            (At::Osc | At::Eight, _, _) => At::Ground,
        };
        (next, ReadLinked::No)
    }
}

/// Where each introducer of `stream` ends: the index of its `;`.
fn introducer_ends(stream: &[u8]) -> Vec<usize> {
    let mut at = At::Ground;
    let mut ends = Vec::new();
    for (i, &b) in StreamByte::slice_from(stream).iter().enumerate() {
        let (next, end) = at.step(b);
        at = next;
        if end == ReadLinked::Yes {
            ends.push(i);
        }
    }
    ends
}

/// The `:` of each `file://` in `w`, whatever byte comes before it.
fn file_colons(w: &[u8]) -> Vec<usize> {
    (4..w.len().saturating_sub(2))
        .filter(|&i| {
            w[i] == b':' && w[i - 4..i].eq_ignore_ascii_case(b"file") && &w[i + 1..i + 3] == b"//"
        })
        .collect()
}

/// **What the sniff must answer for `read` of `stream`**: whether an
/// introducer ends, or a `file://` lies whole, in its window -- the
/// `SNIFF_CARRY` bytes before it, or as many as the stream has, and the
/// read.
fn oracle(stream: &[u8], ends: &[usize], read: Range<usize>) -> ReadLinked {
    let from = read.start.saturating_sub(SNIFF_CARRY);
    let linked = ends.iter().any(|&p| (from..read.end).contains(&p))
        || !file_colons(&stream[from..read.end]).is_empty();
    match linked {
        true => ReadLinked::Yes,
        false => ReadLinked::No,
    }
}

/// One hit of the two-pass scan the sniff replaces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    /// A pattern that ends in the read.
    Read,
    /// A pattern whole in the seven carried bytes: the read before ended
    /// it, and that read's hit is [`Hit::Read`].
    Carried,
}

/// **The two-pass scan the sniff replaces**, a byte at a time: seven bytes
/// carried, the carried bytes and the read's first seven scanned, then the
/// read.
#[derive(Default)]
struct Today {
    tail: Vec<u8>,
}

impl Today {
    /// Where `b` holds an `OSC 8` introducer as contiguous bytes or a
    /// `file://` not after a `;`, each by the index of its last byte.
    fn ends(b: &[u8]) -> Vec<usize> {
        let colons: Vec<usize> = file_colons(b)
            .into_iter()
            .filter(|&i| i < 5 || b[i - 5] != b';')
            .collect();
        (0..b.len())
            .filter_map(|at| {
                let rest = &b[at + 1..];
                match b[at] {
                    0x1b if rest.starts_with(b"]8;") => Some(at + 3),
                    0x9d if rest.starts_with(b"8;") => Some(at + 2),
                    _ if colons.contains(&at) => Some(at + 2),
                    _ => None,
                }
            })
            .collect()
    }

    fn read(&mut self, stream: &[u8], read: Range<usize>) -> Vec<Hit> {
        let bytes = &stream[read];
        let head = &bytes[..bytes.len().min(7)];
        let seam = [self.tail.as_slice(), head].concat();
        let held = self.tail.len();
        let mut hits: Vec<Hit> = Today::ends(&seam)
            .iter()
            .map(|&e| match e < held {
                true => Hit::Carried,
                false => Hit::Read,
            })
            .collect();
        hits.extend(Today::ends(bytes).iter().map(|_| Hit::Read));
        self.tail.extend_from_slice(bytes);
        self.tail.drain(..self.tail.len().saturating_sub(7));
        hits
    }
}

/// One read of a stream, sniffed, with what the oracle and the scan it
/// replaces say of it, and the steps it took.
struct Read {
    span: Range<usize>,
    got: ReadLinked,
    steps: WalkSteps,
    want: ReadLinked,
    today: Vec<Hit>,
    token_at: Option<BytePos>,
    want_token_at: Option<BytePos>,
}

/// The window's token in these tests.
const TOKEN: &[u8] = b"0123456789abcdef";

/// The shell's `D` with the window's token.
const SHELLS_D: &[u8] = b"\x1b]133;D;0;baud_token=0123456789abcdef\x07";

/// **Where the read `read` of `stream` is cut**: at the first [`SHELLS_D`]
/// that ends in it -- the only `D` with the window's token the streams
/// here spell -- when that one also begins in it.
fn cut_at(stream: &[u8], read: Range<usize>) -> Option<BytePos> {
    let first = stream
        .windows(SHELLS_D.len())
        .enumerate()
        .filter(|(_, w)| *w == SHELLS_D)
        .map(|(p, _)| p)
        .find(|&p| read.contains(&(p + SHELLS_D.len() - 1)))?;
    first.checked_sub(read.start).map(BytePos::from_index)
}

/// Each read of `stream` cut at `cuts`, sniffed.
fn sniff_all(stream: &[u8], cuts: &[usize]) -> Vec<Read> {
    let ends = introducer_ends(stream);
    let mut sniff = LinkSniff::new(ShellTokenHex::parse(TOKEN));
    let mut today = Today::default();
    let mut bounds: Vec<usize> = cuts
        .iter()
        .copied()
        .filter(|&c| c > 0 && c < stream.len())
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    bounds.push(stream.len());
    let mut s = 0;
    let mut reads = Vec::new();
    for e in bounds {
        let mut steps = WalkSteps::ZERO;
        let sniffed = sniff.read(&stream[s..e], &mut steps);
        reads.push(Read {
            span: s..e,
            got: sniffed.linked,
            steps,
            want: oracle(stream, &ends, s..e),
            today: today.read(stream, s..e),
            token_at: sniffed.token_at,
            want_token_at: cut_at(stream, s..e),
        });
        s = e;
    }
    reads
}

/// Every read of `stream` cut at `cuts` against the oracle, the bound and the
/// scan it replaces, every pattern that scan found ending in a read linking
/// that read; `None` when all hold.
fn check(stream: &[u8], cuts: &[usize]) -> Option<String> {
    for r in sniff_all(stream, cuts) {
        let len = r.span.len();
        let what = format!("{:?} read {:?}", stream.escape_ascii().to_string(), r.span);
        if r.got != r.want {
            return Some(format!(
                "{what}: sniffed {:?}, the stream reads {:?}",
                r.got, r.want
            ));
        }
        if r.token_at != r.want_token_at {
            return Some(format!(
                "{what}: the shell's D at {:?}, the stream's at {:?}",
                r.token_at.map(BytePos::index),
                r.want_token_at.map(BytePos::index)
            ));
        }
        if usize::try_from(r.steps.get()).expect("steps") > len + SNIFF_CARRY {
            return Some(format!("{what}: {} steps over {len} bytes", r.steps.get()));
        }
        if r.today.contains(&Hit::Read) && r.got == ReadLinked::No {
            return Some(format!(
                "{what}: the scan it replaces linked it, the sniff did not"
            ));
        }
    }
    None
}

/// What `s` says of `bytes`, its steps dropped.
fn linked(s: &mut LinkSniff, bytes: &[u8]) -> ReadLinked {
    let mut steps = WalkSteps::ZERO;
    s.read(bytes, &mut steps).linked
}

/// The patterns the cases embed.
const PATTERNS: [&[u8]; 13] = [
    b"\x1b]8;",
    b"\x9d8;",
    b"\x1b]8\x01;",
    b"\x1b\x01]8;",
    b"\x1b]\x018;",
    b"\x1b\xdd\xb8\xbb",
    b"file://",
    b"FILE://",
    b";file://",
    b"profile:12",
    b"Makefile:12:",
    b"\x1b]9;",
    b"\x1b]133;D;0;baud_token=0123456789abcdef\x07",
];

/// **Each read walks its bytes and the carried ones once, at most its length
/// and `SNIFF_CARRY`, and answers as the stream read a byte at a time
/// does**: every pattern at every offset of a run of up to 79 bytes, read
/// whole and cut in two at every byte, so an introducer stands at every lane
/// of a sixteen-byte block and past the last whole one. Named break: scan
/// the read's first `SNIFF_CARRY` bytes again after the carried ones -- the
/// bound fails at the first cut; pass a block whose first lane is no link
/// -- the introducer in its later lane is missed.
#[test]
fn a_sniff_reads_each_byte_once() {
    let mut asked = 0;
    for p in PATTERNS {
        for len in p.len()..80usize {
            for at in 0..=len - p.len() {
                let mut b = vec![b'a'; len];
                b[at..at + p.len()].copy_from_slice(p);
                for cut in 0..len {
                    if let Some(e) = check(&b, &[cut]) {
                        panic!("{e}");
                    }
                    asked += 1;
                }
            }
        }
    }
    assert!(asked > 100_000, "{asked}");
    let b = b"a:b \x1b]9; file://h/aaaaaaaaaaaaaaaaaaaa";
    assert_eq!(
        linked(&mut LinkSniff::new(None), b),
        ReadLinked::Yes,
        "a lane that is no link, then a link in the same block"
    );
}

/// **A `file://` is linked whatever byte comes before it, at any split**:
/// printed text `x;file:///tmp/a`, `OSC 7`'s folder report and `see
/// file://h/x`, read whole, cut once at every byte and read a byte at a
/// time, link the read holding the second `/` of the `file://`, every read
/// as the stream reads and within its bound. Named break: skip a `file://`
/// whose byte before `file` is `;` -- `x;file:///tmp/a` read whole is
/// unlinked.
#[test]
fn a_file_uri_after_a_semicolon_is_linked_at_every_split() {
    for text in [
        &b"x;file:///tmp/a"[..],
        b"\x1b]7;file://h/home/x\x07",
        b"see file://h/home/x\r\n",
    ] {
        let colon = text.iter().position(|&b| b == b':').expect("a colon");
        let mut cuts: Vec<Vec<usize>> = vec![vec![]];
        cuts.extend((1..text.len()).map(|c| vec![c]));
        cuts.push((1..text.len()).collect());
        for cut in cuts {
            let what = format!("{:?} cut at {cut:?}", text.escape_ascii().to_string());
            if let Some(e) = check(text, &cut) {
                panic!("{e}");
            }
            let on = sniff_all(text, &cut)
                .into_iter()
                .find(|r| r.span.contains(&(colon + 2)))
                .expect("a read holds the `//`");
            assert_eq!(on.got, ReadLinked::Yes, "{what}");
        }
    }
}

/// **A C0 the parser drops inside an `OSC 8` introducer does not hide it, at
/// any split**: `ESC ] 8 SOH ; ; https://h/x ST` and `ESC SOH ] 8 ; ;
/// https://h/x ST`, cut once at every byte and read a byte at a time, are
/// linked on the read holding the introducer's `;`, every read as the
/// stream reads and within its bound; a `BEL` or a `CAN` between the `8`
/// and the `;` ends or cancels the string there, and that read is not
/// linked. Named break: match `ESC ] 8 ;` as contiguous bytes -- neither is
/// linked; let a `BEL` pass in the string -- the controls are linked.
#[test]
fn a_c0_inside_an_osc8_introducer_is_sniffed_at_every_split() {
    let cases = [
        (&b"\x1b]8\x01;;https://h/x\x1b\\"[..], ReadLinked::Yes),
        (b"\x1b\x01]8;;https://h/x\x1b\\", ReadLinked::Yes),
        (b"\x1b]8\x07;;https://h/x\x1b\\", ReadLinked::No),
        (b"\x1b]8\x18;;https://h/x\x1b\\", ReadLinked::No),
    ];
    // The introducer's `;` is the fifth byte of each.
    let semicolon = 4;
    for (bytes, linked) in cases {
        let mut cuts: Vec<Vec<usize>> = (1..bytes.len()).map(|c| vec![c]).collect();
        cuts.push((1..bytes.len()).collect());
        for cut in cuts {
            let what = format!("{:?} cut at {cut:?}", bytes.escape_ascii().to_string());
            if let Some(e) = check(bytes, &cut) {
                panic!("{e}");
            }
            let on = sniff_all(bytes, &cut)
                .into_iter()
                .find(|r| r.span.contains(&semicolon))
                .expect("a read holds the `;`");
            assert_eq!(on.got, linked, "{what}");
        }
    }
}

/// How a generated introducer is spelled.
#[derive(Clone, Copy, Debug)]
enum Form {
    /// `ESC ] 8 ;`
    Seven,
    /// `9D 8 ;`
    C1,
    /// `ESC DD B8 BB`: `]`, `8` and `;` in GR.
    Gr,
}

/// Lower or upper case.
#[derive(Clone, Copy, Debug)]
enum Case {
    Lower,
    Upper,
}

/// Whose a generated `D` is.
#[derive(Clone, Copy, Debug)]
enum Whose {
    /// The window's shell's, its token whole.
    Shells,
    /// Another token's.
    Other,
    /// The shell's, cancelled inside its token by a `CAN`.
    Cancelled,
}

/// One piece of a generated stream.
#[derive(Clone, Debug)]
enum Atom {
    /// An introducer in a form, a byte put inside it after the byte it
    /// names, and the rest of a link.
    Link(Form, Option<(usize, u8)>),
    /// `OSC 7`'s folder report.
    Folder,
    /// `file://` in a case, after a byte.
    File(u8, Case),
    /// Text from bytes that begin or end patterns.
    Text(Vec<u8>),
    /// A `D` mark: the shell's, another token's, or the shell's cancelled.
    Mark(Whose),
    /// Any one byte.
    Byte(u8),
}

impl Atom {
    fn bytes(&self) -> Vec<u8> {
        match self {
            Atom::Link(form, put) => {
                let mut b: Vec<u8> = match form {
                    Form::Seven => b"\x1b]8;".to_vec(),
                    Form::C1 => b"\x9d8;".to_vec(),
                    Form::Gr => b"\x1b\xdd\xb8\xbb".to_vec(),
                };
                if let Some((at, p)) = put {
                    let i = at % (b.len() - 1) + 1;
                    b.insert(i, *p);
                }
                b.extend_from_slice(b";file:///x\x1b\\x");
                b
            }
            Atom::Folder => b"\x1b]7;file://h/w\x07".to_vec(),
            Atom::File(before, case) => {
                let mut b = vec![*before];
                b.extend_from_slice(match case {
                    Case::Lower => b"file://h/x",
                    Case::Upper => b"FILE://h/x",
                });
                b
            }
            Atom::Text(t) => t.clone(),
            Atom::Mark(Whose::Shells) => SHELLS_D.to_vec(),
            Atom::Mark(Whose::Other) => b"\x1b]133;D;0;baud_token=fedcba9876543210\x07".to_vec(),
            Atom::Mark(Whose::Cancelled) => {
                b"\x1b]133;D;0;baud_token=0123\x18456789abcdef\x07".to_vec()
            }
            Atom::Byte(b) => vec![*b],
        }
    }
}

fn atom() -> impl Strategy<Value = Atom> {
    let form = prop_oneof![Just(Form::Seven), Just(Form::C1), Just(Form::Gr)];
    let put = prop_oneof![
        3 => Just(None),
        2 => (0usize..3, 0u8..0x20).prop_map(Some),
        1 => (0usize..3, any::<u8>()).prop_map(Some),
    ];
    let before = prop_oneof![Just(b';'), Just(b' '), Just(b'x'), Just(0x1b)];
    let case = prop_oneof![Just(Case::Lower), Just(Case::Upper)];
    let text = proptest::collection::vec(
        prop_oneof![
            Just(b'a'),
            Just(b':'),
            Just(b'/'),
            Just(b';'),
            Just(b'f'),
            Just(b'8'),
            Just(b']')
        ],
        0..24,
    );
    let byte = prop_oneof![
        Just(0x1bu8),
        Just(0x9d),
        Just(0x07),
        Just(0x18),
        Just(0x1a),
        Just(0x7f),
        any::<u8>()
    ];
    prop_oneof![
        3 => (form, put).prop_map(|(f, p)| Atom::Link(f, p)),
        1 => Just(Atom::Folder),
        2 => (before, case).prop_map(|(b, c)| Atom::File(b, c)),
        3 => text.prop_map(Atom::Text),
        2 => byte.prop_map(Atom::Byte),
        1 => prop_oneof![Just(Whose::Shells), Just(Whose::Other), Just(Whose::Cancelled)].prop_map(Atom::Mark),
    ]
}

/// A stream and the read sizes it is cut into, repeated to its end.
fn stream() -> impl Strategy<Value = (Vec<u8>, Vec<usize>)> {
    (
        proptest::collection::vec(atom(), 1..12)
            .prop_map(|a| a.iter().flat_map(Atom::bytes).collect()),
        proptest::collection::vec(prop_oneof![1usize..24, 24usize..96], 1..6),
    )
}

/// The cut points of `stream` read `sizes` at a time, cycling.
fn cuts(stream: &[u8], sizes: &[usize]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = 0;
    for &n in sizes.iter().cycle() {
        at += n;
        if at >= stream.len() {
            break;
        }
        out.push(at);
    }
    out
}

proptest! {
    /// **Any stream, read in any sizes, is linked on every read whose window
    /// a byte-at-a-time reading of the stream finds a pattern ending in, and
    /// on no other; on every read that ends a pattern the scan it replaces
    /// found; each read within its bound.** Named
    /// break: carry the introducer's state from the read's end rather than
    /// from where the carried bytes begin -- an introducer whose `;` is
    /// carried is found again from its middle, or not at all; read no GR
    /// byte as its 7-bit byte -- the `DD B8 BB` form is missed.
    #[test]
    fn every_read_is_linked_where_the_stream_holds_a_link((bytes, sizes) in stream()) {
        let c = cuts(&bytes, &sizes);
        if let Some(e) = check(&bytes, &c) {
            prop_assert!(false, "{}", e);
        }
    }
}

/// **The generator reaches every introducer form split across reads, a C0
/// inside one, the GR form, a `9D`, and `file://` before and after a `;`,
/// whole and split**: 2,000 streams from a fixed seed. Named break: put no
/// byte inside an introducer -- no C0 stands inside one.
#[test]
fn the_generator_reaches_split_introducers_c0s_inside_them_and_split_file_uris() {
    let mut runner = TestRunner::deterministic();
    let s = stream();
    let names = [
        "an ESC introducer split",
        "a 9D introducer split",
        "a C0 inside an introducer",
        "the GR form",
        "a 9D",
        "file://",
        "file:// split",
        ";file://",
        ";file:// split at its f",
        "the shell's D whole in a read",
        "the shell's D split",
    ];
    let mut reached = [false; 11];
    for _ in 0..2000 {
        let (bytes, sizes) = s.new_tree(&mut runner).expect("a stream").current();
        let c = cuts(&bytes, &sizes);
        for p in introducer_ends(&bytes) {
            let Some(o) = (0..p).rev().find(|&i| bytes[i] == 0x1b || bytes[i] == 0x9d) else {
                continue;
            };
            if c.iter().any(|&k| o < k && k <= p) {
                reached[usize::from(bytes[o] == 0x9d)] = true;
            }
            reached[2] |= bytes[o + 1..p].iter().any(|&b| b < 0x20);
            reached[3] |= bytes[o + 1..=p].iter().any(|&b| b >= 0xa0);
        }
        reached[4] |= bytes.contains(&0x9d);
        for i in file_colons(&bytes) {
            reached[5] = true;
            reached[6] |= c.iter().any(|&k| i - 4 < k && k <= i + 2);
        }
        for i in 5..bytes.len().saturating_sub(2) {
            if bytes[i - 5..i + 3].eq_ignore_ascii_case(b";file://") {
                reached[7] = true;
                reached[8] |= c.contains(&(i - 4));
            }
        }
        for (p, _) in bytes
            .windows(SHELLS_D.len())
            .enumerate()
            .filter(|(_, w)| *w == SHELLS_D)
        {
            match c.iter().any(|&k| p < k && k < p + SHELLS_D.len()) {
                true => reached[10] = true,
                false => reached[9] = true,
            }
        }
    }
    let missed: Vec<_> = names
        .iter()
        .zip(reached)
        .filter(|(_, r)| !r)
        .map(|(n, _)| n)
        .collect();
    assert!(missed.is_empty(), "never reached: {missed:?}");
}

/// What a sniff for the window's token finds of `reads` in turn: where
/// each is cut, and whether each walked within its bound.
fn cuts_of(token: Option<ShellTokenHex>, reads: &[&[u8]]) -> Vec<Option<usize>> {
    let mut s = LinkSniff::new(token);
    reads
        .iter()
        .map(|r| {
            let mut steps = WalkSteps::ZERO;
            let at = s.read(r, &mut steps).token_at;
            let walked = usize::try_from(steps.get()).expect("steps");
            assert!(walked <= r.len() + SNIFF_CARRY, "{walked} over {}", r.len());
            at.map(BytePos::index)
        })
        .collect()
}

/// The `D` the window's shell sends, with `options` after its letter.
fn d_with(options: &str) -> Vec<u8> {
    format!("\x1b]133;D;{options}\x07").into_bytes()
}

/// **The read is cut at the shell's `D`, at its `ESC`, only where the whole
/// mark is in the read and its first `baud_token=` option with a value is
/// the window's token**: with `BEL` or `ST` after it; at the second `ESC`
/// of `ESC ESC ]`; after another `OSC` its `ESC` abandons; the first of two;
/// past an empty token option; and with the token in a field ending inside
/// the body's first 512 bytes. No cut for another token, a token with more
/// digits, a first option with another token, a `C` with the token, a mark
/// a `CAN` cancels, a token past the first 512 bytes or ending a longer
/// body, or a sniff that holds no token. Cut at every byte, a stream is cut
/// only on the read that holds the whole mark; and a read ending a `D`
/// begun in the read before is not cut, though a whole one follows in it.
/// Named break: cut at any `D` holding the token, wherever it stands --
/// the first option with another token and the token past 512 bytes cut;
/// keep the first `D` a read ends from blocking a later one -- the read
/// after a split `D` is cut.
#[test]
fn the_shells_d_is_found_where_it_begins_and_ends_in_the_read() {
    let token = ShellTokenHex::parse(TOKEN);
    let tok = std::str::from_utf8(TOKEN).expect("hex");
    let ours = d_with(&format!("0;baud_token={tok}"));
    let one = |bytes: &[u8]| cuts_of(token, &[bytes])[0];
    let pad = format!("pad={}", "x".repeat(600));
    let cases: [(Vec<u8>, Option<usize>); 16] = [
        ([b"$ ".as_slice(), &ours, b"% "].concat(), Some(2)),
        (
            format!("$ \x1b]133;D;0;baud_token={tok}\x1b\\% ").into_bytes(),
            Some(2),
        ),
        ([b"\x1b".as_slice(), &ours].concat(), Some(1)),
        ([b"\x1b]0;title".as_slice(), &ours].concat(), Some(9)),
        ([b"x".as_slice(), &ours, &ours].concat(), Some(1)),
        (d_with(&format!("0;baud_token=;baud_token={tok}")), Some(0)),
        (d_with(&format!("0;baud_token={tok};{pad}")), Some(0)),
        (d_with("0;baud_token=fedcba9876543210"), None),
        (d_with(&format!("0;baud_token={tok}0")), None),
        (
            d_with(&format!("0;baud_token=fedcba9876543210;baud_token={tok}")),
            None,
        ),
        (
            format!("\x1b]133;C;baud_token={tok}\x07").into_bytes(),
            None,
        ),
        (
            format!("\x1b]133;D;0;baud_token={tok}\x18\x07").into_bytes(),
            None,
        ),
        (d_with(&format!("0;{pad};baud_token={tok};aid=1")), None),
        (d_with(&format!("0;{pad};baud_token={tok}")), None),
        (
            [d_with(&format!("0;baud_token={tok}")), d_with("1")].concat(),
            Some(0),
        ),
        ([b"\x1b]133;D;0".as_slice(), &ours].concat(), Some(9)),
    ];
    for (bytes, want) in &cases {
        let shown = bytes.escape_ascii().to_string();
        assert_eq!(one(bytes), *want, "{shown}");
        assert_eq!(cuts_of(None, &[bytes]), [None], "no token held: {shown}");
    }
    // Cut at every byte: the whole mark in one read, or no cut.
    let stream = [b"$ ".as_slice(), &ours, b"% "].concat();
    let mark = 2..2 + ours.len();
    for k in 1..stream.len() {
        let (a, b) = stream.split_at(k);
        let want = match (k <= mark.start, k >= mark.end) {
            (true, _) => [None, Some(mark.start - k)],
            (_, true) => [Some(mark.start), None],
            _ => [None, None],
        };
        assert_eq!(cuts_of(token, &[a, b]), want, "cut at {k}");
        // The `D` split, a whole one after it in the second read.
        let again = [b, &ours].concat();
        let first_split = mark.start < k && k < mark.end;
        let second = cuts_of(token, &[a, &again])[1];
        let want = match (first_split, k <= mark.start) {
            (true, _) => None,
            (false, true) => Some(mark.start - k),
            (false, false) => Some(b.len()),
        };
        assert_eq!(second, want, "cut at {k}, a whole D after it");
    }
}
