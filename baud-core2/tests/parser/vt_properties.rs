//! Property tests over *generated VT programs*, not over random bytes.
//!
//! `properties.rs` fuzzes with `any::<u8>()`, which is the right shape for
//! "must not panic" but a poor one for protocol coverage: uniform random bytes
//! essentially never spell a well-formed `CSI ... m`, so the operations added
//! for Tier 0-2 were never reached by a property at all. This file generates
//! syntactically valid sequences instead, so the invariants below are asserted
//! against the parser doing real work rather than against the ignore path.
//!
//! Each property is written so it needs no second implementation of the
//! terminal to check against -- they are differential (two ways of running the
//! same program must agree) or algebraic (erasing twice equals erasing once).
//! A model would be a second terminal, and a second terminal has its own bugs.

use baud_core2::*;
use baud_domain::ParamIdx;
use proptest::prelude::*;

// ── a generator for well-formed sequences ───────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum Op {
    /// `n` copies of a printable character.
    Text(u8, u8),
    /// An SGR with *many* parameters, crossing `ParamIdx::CAPACITY`.
    ///
    /// `Op::Sgr` emits exactly one parameter, so no generated program ever
    /// carried a long list -- which is why `generated_programs_are_chunk_
    /// invariant` passed for the whole time the two feeds disagreed about
    /// what to do past the capacity bound. A property is only as good as the
    /// shapes its generator can reach.
    SgrMany(u8, u8),
    /// An SGR whose parameter list has *holes*: `CSI 1;;4;;;7 m`.
    ///
    /// An omitted parameter means its default, which for SGR is 0 (reset).
    /// Nothing in this generator produced one, so the whole
    /// empty-parameter path -- `params[i] = None` and every `unwrap_or`
    /// behind it -- was reachable only from hand-written cases.
    SgrHoles(u8, u8),
    /// A CUP whose parameters exceed `Param`'s `u16`.
    ///
    /// `Param::push` saturates at 65535 rather than wrapping. The largest
    /// value this generator could otherwise build is three digits, so the
    /// saturation was never taken and a wrapping implementation would have
    /// passed every property here.
    CupHuge(u8),
    /// A C0 control *inside* a CSI parameter string.
    ///
    /// `[[source: williams]]` executes it and the sequence continues, which
    /// `a_c0_inside_a_csi_does_not_cancel_it` pins for one case. The
    /// generator could not interleave one, so the interaction between an
    /// executed C0 and a half-parsed sequence had no property coverage.
    CsiWithC0(u8, u8),
    /// The eleven `done` forms the generator could not reach, found by
    /// enumerating `docs/vt-coverage.md` against the corpus rather than by
    /// guessing. `report_generator_reach` is that enumeration; it now asserts.
    Cnl(u8),
    Cpl(u8),
    Cht(u8),
    Cbt(u8),
    /// HPA, `CSI Pn \``.
    Hpa(u8),
    Hpr(u8),
    Vpr(u8),
    /// TBC, `CSI Ps g`.
    Tbc(u8),
    /// SM / RM in the *unprefixed* namespace -- IRM and LNM. `Decset`/`Decrst`
    /// only ever emitted the `?` form, so plain `h` and `l` were never
    /// generated at all, and SM/RM is an operation this project has already
    /// had sitting in the parsed-and-dropped state.
    Sm(u16),
    Rm(u16),
    /// DECSTR, `CSI ! p`. The intermediate path -- `csi_intermediate` was one
    /// of the four missing states whose absence was a bug.
    Decstr,
    /// The nine `ESC` forms `report_escape_reach` found unreachable.
    Deckpam,
    Deckpnm,
    /// HTS, `ESC H` — set a tab stop at the cursor.
    Hts,
    /// RIS, `ESC c` — a hard reset.
    Ris,
    /// S7C1T / S8C1T, `ESC SP F` and `ESC SP G`.
    S7c1t,
    S8c1t,
    /// DECALN, `ESC # 8`.
    Decaln,
    /// `ESC % @` and `ESC % G` — the input encoding.
    SelectLatin1,
    SelectUtf8,
    /// `CSI <final>` with its parameters **omitted**, so the operation takes
    /// its default.
    ///
    /// Every numeric op above always wrote a number, so of the thirty-one
    /// finals the generator emits, only `c` was ever sent bare. The default is
    /// a distinct code path per operation -- CUP defaults to 1;1, SU to 1, SGR
    /// to 0 -- and this project has already had an off-by-one in exactly that
    /// kind of arithmetic (CPR).
    Bare(u8, u8),
    /// `CSI 99999 <final>`, so `Param::push`'s saturation is taken on a real
    /// operation rather than only on CUP.
    Extreme(u8, u8),
    Cup(u8, u8),
    Cuu(u8),
    Cud(u8),
    Cuf(u8),
    Cub(u8),
    Cha(u8),
    Vpa(u8),
    Cr,
    Lf,
    Tab,
    Bs,
    Nel,
    Ind,
    Ri,
    /// One SGR parameter from the set baud claims.
    Sgr(u8),
    /// DECSCA `Ps " q`.
    Decsca(u8),
    El(u8),
    Ed(u8),
    Decsel(u8),
    Decsed(u8),
    Il(u8),
    Dl(u8),
    Ich(u8),
    Dch(u8),
    Ech(u8),
    Su(u8),
    Sd(u8),
    Stbm(u8, u8),
    Decset(u16),
    Decrst(u16),
    Save,
    Restore,
    Rep(u8),
    Dsr(u8),
    Da,
    /// DECSCUSR, `CSI Ps SP q`.
    Decscusr(u8),
    /// DECRQM, `CSI Ps $ p` / `CSI ? Ps $ p`.
    Decrqm(bool, u16),
    /// The ISO 8613-6 colon form of direct colour.
    SgrColonRgb(u8, u8, u8, u8),
    /// The colon form of indexed colour.
    SgrColonIdx(u8, u8),
    /// `OSC Ps ; Pt` with either terminator.
    Osc(u16, u8, bool),
    /// `ESC ( ) * + C` — designate a character set.
    Designate(u8, u8),
    /// SO / SI.
    So,
    Si,
    /// DECSLRM, `CSI Pl ; Pr s` (DECSLRM only when DECLRMM is on).
    Decslrm(u8, u8),
    /// DECIC / DECDC.
    Decic(u8),
    Decdc(u8),
    /// DECRQSS, `DCS $ q Pt ST`.
    Decrqss(u8),
    /// `OSC 8` open and close.
    Link(u8),
    Unlink,
    /// A control string — SOS, PM, APC or a non-DECRQSS DCS — carrying a
    /// payload that would be commands if it were executed.
    ///
    /// The generator emitted none of these. `Op::Osc` covers `OSC`, and
    /// `Op::Decrqss` covers the one DCS form baud answers, but the
    /// `sos_pm_apc_string` family had no property coverage at all — and it is
    /// the family where the bug is not "an operation is missing" but "the
    /// payload was executed as commands", which is what
    /// `apc_payload_is_not_executed` pins for one case.
    ///
    /// Terminated with the 7-bit `ESC \` only. `9C` is ST's 8-bit spelling,
    /// but under UTF-8 it is a continuation byte and does *not* terminate —
    /// correctly, and `docs/vt-coverage.md` records that as deliberate — so
    /// generating it would swallow the rest of the program and break the
    /// composability the other properties rely on.
    ControlString(u8, u8),
    /// A run of non-ASCII text, valid or malformed.
    ///
    /// `Op::Text` emits `0x21..0x7f`, so **no generated program contained a
    /// multi-byte sequence at all** — and `properties.rs` uses `any::<u8>()`,
    /// which by this file's own argument essentially never spells a
    /// well-formed cluster any more than it spells a well-formed CSI.
    ///
    /// That left the half of the job `CLAUDE.md` calls "what the bytes between
    /// the sequences mean" — UTF-8 decoding, width, grapheme clustering —
    /// covered only by hand-written cases. Chunk invariance is where it bites:
    /// a multi-byte sequence split across a read boundary is the classic
    /// terminal bug, and this suite could not produce one.
    Utf8(u8),
    /// A self-contained excursion into **VT52 mode**: `CSI ? 2 l`, some VT52
    /// commands, then `ESC <` back to ANSI.
    ///
    /// DECANM was not in `MODES`, so no generated program ever left ANSI, and
    /// the whole VT52 command set — fourteen operations including `ESC Y`
    /// direct addressing, which needs its own parser sub-state to collect two
    /// bias-`0x20` coordinate bytes — was unreachable from every property here.
    ///
    /// Self-contained on purpose. VT52 changes how *all* subsequent bytes
    /// parse: `ESC [` is not a CSI introducer there. A program left in VT52
    /// would make every composing property diverge for a reason that is not a
    /// bug, the same way RIS and a pending UTF-8 sequence did. Entering and
    /// leaving in one op keeps the terminal in a known state at the boundary
    /// while still driving the sub-state inside it.
    Vt52Block(u8),
}

/// SGR parameters baud implements, so a generated program exercises styles
/// rather than the "unknown attribute" break.
const SGR: [u8; 34] = [
    0, 1, 2, 3, 4, 5, 7, 8, 9, 22, 24, 27, 30, 39, 41, 49, 90, 97, 100, 107,
    // The attributes ECMA-48 § 8.3.117 defines past 19, which the table
    // omitted -- so no generated program ever set fraktur, framed, overlined
    // or an ideogram aspect, and none exercised the parameters that clear two
    // or five bits at once (23, 54, 65). They were implemented and then only
    // tested by hand-written cases.
    20, 23, 26, 50, 51, 52, 53, 54, 55, 60, 61, 62, 63, 64,
];

/// Private modes baud acts on, plus two it deliberately ignores -- an ignored
/// mode must stay inert, and that is worth generating too.
/// C0 controls that are legal inside a CSI and are *executed* there.
///
/// Not the ones that abort it (`CAN`, `SUB`, `ESC`) -- those end the sequence
/// by design and would make this generate something other than what it means.
const C0: [u8; 5] = [0x07, 0x08, 0x09, 0x0a, 0x0d];

/// Modes in the unprefixed namespace: the two baud acts on, and two it does
/// not -- an unimplemented mode must stay inert, which is worth generating.
const ANSI_MODES: [u16; 4] = [4, 20, 2, 12];

/// Control-string payloads, chosen so that executing one would be visible.
///
/// The first four contain no `ESC` and must be consumed in silence. The last
/// two do: `[[source: williams]]` has a bare `ESC` *cancel* a control string,
/// so those legitimately end the string and run what follows — which is
/// behaviour worth generating rather than avoiding, since both feeds have to
/// agree on where the cancel happened.
/// Non-ASCII runs: valid clusters of every width class, and the malformed
/// forms whose recovery has already been wrong here.
///
/// Raw bytes rather than `&str` because half of these are not valid UTF-8 and
/// could not be written as one. The overlong `E0 80 AF` is the path-traversal
/// form that used to render `/`; `C2 41` is the truncation whose recovery ate
/// the valid successor byte.
/// Every designator `Charset::from_final` maps to something, plus two it does
/// not.
///
/// The generator offered four -- `0 B A <` -- so eighteen of baud's twenty-two
/// were never designated by any generated program, and NRCS *translation* is
/// the path where this session already found a bug: `put_ascii_run` tested
/// `flags == NONE` before the charset, so German text took the ASCII fast path
/// untranslated. `every_designator_is_reachable` keeps this in step with
/// `from_final` rather than with a hand-written list.
const DESIGNATORS: [u8; 24] = [
    b'0', b'B', b'A', b'C', b'5', b'H', b'7', b'K', b'Q', b'9', b'R', b'f', b'Y', b'Z', b'4', b'=',
    b'`', b'E', b'6', b'<', b'>', b'I', b'J',
    // Not implemented: must designate `Charset::Other` and stay inert.
    b'~',
];

/// What to do while in VT52 mode.
///
/// `ESC Y` takes two bias-`0x20` bytes, so `\x1bY\x20\x20` is row 0 column 0
/// and `\x1bY\x28\x2f` is row 8 column 15 -- the coordinates are the point,
/// because collecting them is a parser sub-state nothing else reaches.
/// `ESC Z` is answered, so `replies_are_well_formed` sees VT52 replies too.
const VT52_RUNS: [&[u8]; 8] = [
    b"\x1bA\x1bB\x1bC\x1bD",
    b"\x1bH\x1bI",
    b"\x1bY\x20\x20text",
    b"\x1bY\x28\x2ftext",
    b"\x1bJ",
    b"\x1bK",
    b"\x1bF\x1bG",
    b"\x1bZ",
];

const UTF8_RUNS: [&[u8]; 14] = [
    "é".as_bytes(),
    "→".as_bytes(),
    "🚀".as_bytes(),
    "世界".as_bytes(),
    // A base with a combining mark, which must attach rather than take a cell.
    "e\u{301}a\u{308}".as_bytes(),
    // ZWJ emoji: one cluster, the columns of one.
    "👨\u{200d}👩\u{200d}👧".as_bytes(),
    // Regional indicators — EAW Neutral, so width from East Asian Width alone
    // would make this half a flag.
    "🇯🇵".as_bytes(),
    // Devanagari conjunct: marks that the old hand-written table gave columns.
    "क\u{93f}".as_bytes(),
    "עִבְרִית".as_bytes(),
    // Malformed, from here down.
    &[0xe0, 0x80, 0xaf],
    &[0xc2, 0x41, 0x42],
    &[0xe2, 0x86],
    &[0x80, 0x80],
    &[0xed, 0xa0, 0x80],
];

const STRING_PAYLOADS: [&str; 6] = [
    "Gpayload;with=params,and:punctuation",
    "\x07\x08\x09 bell backspace tab inside a string",
    "",
    "q#0;2;0;0;0#0~~@@vv@@~~@@~~$#1?????@@??@@??--#2!14@",
    "cancelled\x1b[2Jby-an-escape",
    "\x1b[31mred",
];

/// Every CSI final this generator emits carrying a parameter.
///
/// `every_parameterised_final_is_also_sent_bare_and_saturating` keeps this in
/// step with the
/// ops above: a new parameterised operation that is not listed here is one
/// whose default path nothing generates.
/// `(intermediate, final)`; a zero intermediate means none.
///
/// The three with one — DECSCUSR `SP q`, DECIC `' }` and DECDC `' ~` — are why
/// this is a pair and not a byte. Their bare spelling still carries the
/// intermediate; dropping it would name a different operation, and the first
/// version of this table did exactly that until
/// `every_parameterised_final_is_also_sent_bare_and_saturating` named all
/// three.
const PARAMETERISED: [(u8, u8); 36] = [
    (0, b'@'),
    (0, b'A'),
    (0, b'B'),
    (0, b'C'),
    (0, b'D'),
    (0, b'E'),
    (0, b'F'),
    (0, b'G'),
    (0, b'H'),
    (0, b'I'),
    (0, b'J'),
    (0, b'K'),
    (0, b'L'),
    (0, b'M'),
    (0, b'P'),
    (0, b'S'),
    (0, b'T'),
    (0, b'X'),
    (0, b'Z'),
    (0, b'`'),
    (0, b'a'),
    (0, b'b'),
    (0, b'd'),
    (0, b'e'),
    (0, b'g'),
    (0, b'h'),
    (0, b'l'),
    (0, b'm'),
    (0, b'n'),
    (0, b'r'),
    (0, b's'),
    (0x20, b'q'),
    (b'\'', b'}'),
    (b'\'', b'~'),
    (b'"', b'q'),
    (b'$', b'p'),
];

const MODES: [u16; 11] = [6, 7, 25, 69, 1000, 1002, 1003, 1006, 1049, 2004, 2026];

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0x21u8..0x7f, 1u8..40).prop_map(|(c, n)| Op::Text(c, n)),
        (0u8..UTF8_RUNS.len() as u8).prop_map(Op::Utf8),
        (0u8..VT52_RUNS.len() as u8).prop_map(Op::Vt52Block),
        (0u8..60, 0u8..210).prop_map(|(r, c)| Op::Cup(r, c)),
        (0u8..60).prop_map(Op::Cuu),
        (0u8..60).prop_map(Op::Cud),
        (0u8..210).prop_map(Op::Cuf),
        (0u8..210).prop_map(Op::Cub),
        (0u8..210).prop_map(Op::Cha),
        (0u8..60).prop_map(Op::Vpa),
        Just(Op::Cr),
        Just(Op::Lf),
        Just(Op::Tab),
        Just(Op::Bs),
        Just(Op::Nel),
        Just(Op::Ind),
        Just(Op::Ri),
        prop::sample::select(&SGR[..]).prop_map(Op::Sgr),
        // Deliberately spans the capacity bound: short lists, lists exactly at
        // it, and lists well past it.
        (1u8..2 * ParamIdx::CAPACITY as u8, 0u8..20).prop_map(|(n, off)| Op::SgrMany(n, off)),
        (1u8..12, 0u8..20).prop_map(|(n, off)| Op::SgrHoles(n, off)),
        (0u8..4).prop_map(Op::CupHuge),
        (prop::sample::select(&C0[..]), 0u8..6).prop_map(|(c, at)| Op::CsiWithC0(c, at)),
        (0u8..60).prop_map(Op::Cnl),
        (0u8..60).prop_map(Op::Cpl),
        (0u8..10).prop_map(Op::Cht),
        (0u8..10).prop_map(Op::Cbt),
        (0u8..210).prop_map(Op::Hpa),
        (0u8..210).prop_map(Op::Hpr),
        (0u8..60).prop_map(Op::Vpr),
        (0u8..4).prop_map(Op::Tbc),
        prop::sample::select(&ANSI_MODES[..]).prop_map(Op::Sm),
        prop::sample::select(&ANSI_MODES[..]).prop_map(Op::Rm),
        Just(Op::Decstr),
        Just(Op::Deckpam),
        Just(Op::Deckpnm),
        Just(Op::Hts),
        Just(Op::Ris),
        Just(Op::S7c1t),
        Just(Op::S8c1t),
        Just(Op::Decaln),
        Just(Op::SelectLatin1),
        Just(Op::SelectUtf8),
        prop::sample::select(&PARAMETERISED[..]).prop_map(|(i, f)| Op::Bare(i, f)),
        prop::sample::select(&PARAMETERISED[..]).prop_map(|(i, f)| Op::Extreme(i, f)),
        (0u8..3).prop_map(Op::Decsca),
        (0u8..3).prop_map(Op::El),
        (0u8..3).prop_map(Op::Ed),
        (0u8..3).prop_map(Op::Decsel),
        (0u8..3).prop_map(Op::Decsed),
        (0u8..8).prop_map(Op::Il),
        (0u8..8).prop_map(Op::Dl),
        (0u8..8).prop_map(Op::Ich),
        (0u8..8).prop_map(Op::Dch),
        (0u8..8).prop_map(Op::Ech),
        (0u8..8).prop_map(Op::Su),
        (0u8..8).prop_map(Op::Sd),
        (0u8..60, 0u8..60).prop_map(|(a, b)| Op::Stbm(a, b)),
        prop::sample::select(&MODES[..]).prop_map(Op::Decset),
        prop::sample::select(&MODES[..]).prop_map(Op::Decrst),
        Just(Op::Save),
        Just(Op::Restore),
        (0u8..8).prop_map(Op::Rep),
        prop::sample::select(&[5u8, 6, 15][..]).prop_map(Op::Dsr),
        Just(Op::Da),
        (0u8..9).prop_map(Op::Decscusr),
        (any::<bool>(), prop::sample::select(&MODES[..])).prop_map(|(p, m)| Op::Decrqm(p, m)),
        (0u8..3, 0u8..255, 0u8..255, 0u8..255).prop_map(|(i, r, g, b)| Op::SgrColonRgb(i, r, g, b)),
        (prop::sample::select(&[38u8, 48][..]), 0u8..255).prop_map(|(k, n)| Op::SgrColonIdx(k, n)),
        (
            prop::sample::select(&[0u16, 1, 2, 4, 52, 104, 777][..]),
            0u8..8,
            any::<bool>(),
        )
            .prop_map(|(ps, k, st)| Op::Osc(ps, k, st)),
        (
            prop::sample::select(&[b'(', b')', b'*', b'+'][..]),
            prop::sample::select(&DESIGNATORS[..]),
        )
            .prop_map(|(g, c)| Op::Designate(g, c)),
        Just(Op::So),
        Just(Op::Si),
        (0u8..60, 0u8..60).prop_map(|(a, b)| Op::Decslrm(a, b)),
        (0u8..6).prop_map(Op::Decic),
        (0u8..6).prop_map(Op::Decdc),
        (0u8..7).prop_map(Op::Decrqss),
        (0u8..4).prop_map(Op::Link),
        Just(Op::Unlink),
        (
            prop::sample::select(&[b'X', b'^', b'_', b'P'][..]),
            0u8..STRING_PAYLOADS.len() as u8,
        )
            .prop_map(|(k, p)| Op::ControlString(k, p))
    ]
}

fn program() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(op(), 0..60)
}

fn emit(ops: &[Op], out: &mut Vec<u8>) {
    use core::fmt::Write;
    let mut n = String::new();
    for o in ops {
        n.clear();
        match *o {
            Op::Utf8(k) => {
                out.extend_from_slice(UTF8_RUNS[usize::from(k)]);
                continue;
            }
            Op::Vt52Block(k) => {
                out.extend_from_slice(b"\x1b[?2l");
                out.extend_from_slice(VT52_RUNS[usize::from(k)]);
                out.extend_from_slice(b"\x1b<");
                continue;
            }
            Op::Text(c, k) => {
                for _ in 0..k {
                    out.push(c);
                }
                continue;
            }
            Op::Cup(r, c) => write!(n, "\x1b[{r};{c}H").unwrap(),
            Op::SgrHoles(count, off) => {
                n.push_str("\x1b[");
                for k in 0..count as usize {
                    if k > 0 {
                        n.push(';');
                    }
                    // Every third position is left empty, so a list mixes
                    // present and defaulted parameters rather than being all
                    // of one kind.
                    if (k + off as usize) % 3 != 0 {
                        write!(n, "{}", SGR[(k + off as usize) % SGR.len()]).unwrap();
                    }
                }
                n.push('m');
            }
            Op::Cnl(k) => write!(n, "\x1b[{k}E").unwrap(),
            Op::Cpl(k) => write!(n, "\x1b[{k}F").unwrap(),
            Op::Cht(k) => write!(n, "\x1b[{k}I").unwrap(),
            Op::Cbt(k) => write!(n, "\x1b[{k}Z").unwrap(),
            Op::Hpa(k) => write!(n, "\x1b[{k}`").unwrap(),
            Op::Hpr(k) => write!(n, "\x1b[{k}a").unwrap(),
            Op::Vpr(k) => write!(n, "\x1b[{k}e").unwrap(),
            Op::Tbc(k) => write!(n, "\x1b[{k}g").unwrap(),
            Op::Sm(m) => write!(n, "\x1b[{m}h").unwrap(),
            Op::Rm(m) => write!(n, "\x1b[{m}l").unwrap(),
            Op::Decstr => n.push_str("\x1b[!p"),
            Op::Deckpam => n.push_str("\x1b="),
            Op::Deckpnm => n.push_str("\x1b>"),
            Op::Hts => n.push_str("\x1bH"),
            Op::Ris => n.push_str("\x1bc"),
            Op::S7c1t => n.push_str("\x1b F"),
            Op::S8c1t => n.push_str("\x1b G"),
            Op::Decaln => n.push_str("\x1b#8"),
            Op::SelectLatin1 => n.push_str("\x1b%@"),
            Op::SelectUtf8 => n.push_str("\x1b%G"),
            Op::Bare(i, f) => {
                n.push_str("\x1b[");
                if i != 0 {
                    n.push(i as char);
                }
                n.push(f as char);
            }
            Op::Extreme(i, f) => {
                n.push_str("\x1b[99999");
                if i != 0 {
                    n.push(i as char);
                }
                n.push(f as char);
            }
            Op::CupHuge(k) => {
                let big = ["99999", "65535", "65536", "4294967296"][k as usize % 4];
                write!(n, "\x1b[{big};{big}H").unwrap();
            }
            Op::CsiWithC0(c, at) => {
                // `CSI 1;2;3 m` with one C0 byte spliced in among the
                // parameters.
                n.push_str("\x1b[");
                for k in 0..6usize {
                    if k > 0 {
                        n.push(';');
                    }
                    if k == at as usize % 6 {
                        n.push(c as char);
                    }
                    write!(n, "{}", SGR[k % SGR.len()]).unwrap();
                }
                n.push('m');
            }
            Op::Cuu(k) => write!(n, "\x1b[{k}A").unwrap(),
            Op::Cud(k) => write!(n, "\x1b[{k}B").unwrap(),
            Op::Cuf(k) => write!(n, "\x1b[{k}C").unwrap(),
            Op::Cub(k) => write!(n, "\x1b[{k}D").unwrap(),
            Op::Cha(k) => write!(n, "\x1b[{k}G").unwrap(),
            Op::Vpa(k) => write!(n, "\x1b[{k}d").unwrap(),
            Op::Cr => n.push('\r'),
            Op::Lf => n.push('\n'),
            Op::Tab => n.push('\t'),
            Op::Bs => n.push('\x08'),
            Op::Nel => n.push_str("\x1bE"),
            Op::Ind => n.push_str("\x1bD"),
            Op::Ri => n.push_str("\x1bM"),
            Op::Sgr(p) => write!(n, "\x1b[{p}m").unwrap(),
            Op::SgrMany(count, off) => {
                n.push_str("\x1b[");
                for k in 0..count as usize {
                    if k > 0 {
                        n.push(';');
                    }
                    write!(n, "{}", SGR[(k + off as usize) % SGR.len()]).unwrap();
                }
                n.push('m');
            }
            Op::Decsca(p) => write!(n, "\x1b[{p}\"q").unwrap(),
            Op::El(p) => write!(n, "\x1b[{p}K").unwrap(),
            Op::Ed(p) => write!(n, "\x1b[{p}J").unwrap(),
            Op::Decsel(p) => write!(n, "\x1b[?{p}K").unwrap(),
            Op::Decsed(p) => write!(n, "\x1b[?{p}J").unwrap(),
            Op::Il(k) => write!(n, "\x1b[{k}L").unwrap(),
            Op::Dl(k) => write!(n, "\x1b[{k}M").unwrap(),
            Op::Ich(k) => write!(n, "\x1b[{k}@").unwrap(),
            Op::Dch(k) => write!(n, "\x1b[{k}P").unwrap(),
            Op::Ech(k) => write!(n, "\x1b[{k}X").unwrap(),
            Op::Su(k) => write!(n, "\x1b[{k}S").unwrap(),
            Op::Sd(k) => write!(n, "\x1b[{k}T").unwrap(),
            Op::Stbm(a, b) => write!(n, "\x1b[{a};{b}r").unwrap(),
            Op::Decset(m) => write!(n, "\x1b[?{m}h").unwrap(),
            Op::Decrst(m) => write!(n, "\x1b[?{m}l").unwrap(),
            Op::Save => n.push_str("\x1b7"),
            Op::Restore => n.push_str("\x1b8"),
            Op::Rep(k) => write!(n, "\x1b[{k}b").unwrap(),
            Op::Dsr(p) => write!(n, "\x1b[{p}n").unwrap(),
            Op::Da => n.push_str("\x1b[c"),
            Op::Decscusr(p) => write!(n, "\x1b[{p} q").unwrap(),
            Op::Decrqm(priv_, m) => {
                write!(n, "\x1b[{}{m}$p", if priv_ { "?" } else { "" }).unwrap()
            }
            Op::SgrColonRgb(i, r, g, b) => write!(n, "\x1b[38:2:{i}:{r}:{g}:{b}m").unwrap(),
            Op::SgrColonIdx(k, v) => write!(n, "\x1b[{k}:5:{v}m").unwrap(),
            // Payloads that look like the real thing: a title, a colour spec,
            // a base64 blob. Terminated both ways.
            Op::Osc(ps, k, st) => {
                let body = match ps {
                    4 => alloc_spec(k),
                    52 => "c;aGVsbG8=".to_string(),
                    104 => format!("{k}"),
                    _ => "t".repeat(usize::from(k) + 1),
                };
                write!(n, "\x1b]{ps};{body}").unwrap();
                n.push_str(if st { "\x1b\\" } else { "\x07" });
            }
            Op::Designate(g, c) => {
                n.push('\x1b');
                n.push(char::from(g));
                n.push(char::from(c));
            }
            Op::So => n.push('\x0e'),
            Op::Si => n.push('\x0f'),
            Op::Decslrm(a, b) => write!(n, "\x1b[{a};{b}s").unwrap(),
            Op::Decic(k) => write!(n, "\x1b[{k}'}}").unwrap(),
            Op::Decdc(k) => write!(n, "\x1b[{k}'~").unwrap(),
            Op::Decrqss(k) => {
                let pt = ["m", "r", "s", " q", "\"q", "\"p", "zz"][usize::from(k)];
                write!(n, "\x1bP$q{pt}\x1b\\").unwrap();
            }
            Op::Link(k) => write!(n, "\x1b]8;id={k};http://example.com/{k}\x1b\\").unwrap(),
            Op::Unlink => n.push_str("\x1b]8;;\x1b\\"),
            Op::ControlString(kind, p) => {
                n.push('\x1b');
                n.push(kind as char);
                n.push_str(STRING_PAYLOADS[usize::from(p)]);
                n.push_str("\x1b\\");
            }
        }
        out.extend_from_slice(n.as_bytes());
    }
}

/// A plausible `OSC 4` body, including the query form.
fn alloc_spec(k: u8) -> String {
    if k % 4 == 0 {
        format!("{k};?")
    } else {
        format!("{k};rgb:{k:02x}/00/ff")
    }
}

fn bytes_of(ops: &[Op]) -> Vec<u8> {
    let mut v = Vec::new();
    emit(ops, &mut v);
    v
}

fn drive(bytes: &[u8], chunk: usize, mode: OpMode) -> (Vec<PackedCell>, (usize, usize), Vec<u8>) {
    let mut store = Storage::boxed();
    let mut t = Term::new(&mut store, mode);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(READ_CHUNK).min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    let (r, c) = t.cursor();
    let reply = t.reply().to_vec();
    (t.viewport().to_vec(), (r.index(), c.index()), reply)
}

/// The same, through `machine.rs`'s rule walk: one byte, one walk, no
/// scanner and no fast paths.
#[cfg(feature = "rule-walk")]
fn drive_bytewise(
    bytes: &[u8],
    mode: OpMode,
    rules: bool,
) -> (Vec<PackedCell>, (usize, usize), Vec<u8>) {
    let mut store = Storage::boxed();
    let mut t = Term::new(&mut store, mode);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        if rules {
            t.feed_by_rules(base, filled.len());
        } else {
            t.feed_by_graph(base, filled.len());
        }
        t.commit_read(filled.len());
        i += n;
    }
    let (r, c) = t.cursor();
    let reply = t.reply().to_vec();
    (t.viewport().to_vec(), (r.index(), c.index()), reply)
}

fn grid(ops: &[Op]) -> Vec<PackedCell> {
    drive(&bytes_of(ops), 4096, OpMode::Computed).0
}

/// Run `ops`, then `extra`, and return the viewport.
/// Close the decoder before a program is extended.
///
/// A program ending mid-UTF-8-sequence is an *open* state: the decoder holds
/// bytes that the next input either completes or flushes as one U+FFFD. So
/// `grid(ops)` and `grid(ops ++ extra)` are legitimately different, and worse,
/// two different suffixes flush at different moments and under whatever
/// rendition is current -- `[Decsca(1), <E2 86>]` makes one side's flush a
/// *protected* U+FFFD, which selective erase spares and plain erase does not.
///
/// Every composing property below therefore diverges *after* this op, never
/// before it. It is a plain character rather than something clever because it
/// has to be identical on both sides and nothing more.
///
/// `an_incomplete_sequence_at_the_end_renders_nothing_until_flushed` pins the
/// behaviour this compensates for; the behaviour is correct and the properties
/// were the things that were wrong.
const FLUSH: Op = Op::Text(b'.', 1);

fn grid_then(ops: &[Op], extra: &[Op]) -> Vec<PackedCell> {
    let mut v = ops.to_vec();
    v.push(FLUSH);
    v.extend_from_slice(extra);
    grid(&v)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(150))]

    // ── the parser, under valid input ───────────────────────────────────────

    /// Chunk invariance, but over sequences that actually parse.
    ///
    /// The existing byte-level property can only split a sequence it never
    /// generates. This one splits real ones, which is where the boundary bugs
    /// live -- `fast_csi` bails to the byte-at-a-time machine mid-sequence, and
    /// that handover is only exercised when a valid CSI straddles a read.
    ///
    /// `Computed` and `Templated`, not `Runs`: in `Runs` mode `viewport()`
    /// reports representation, which is chunk-dependent by construction. See
    /// `properties.rs::promotion_depends_on_chunking_in_runs_mode`, which pins
    /// the reason.
    #[test]
    /// **The graph must do what the rules do, on every generated program.**
    ///
    /// `the_table_sends_every_byte_where_the_machine_does` proves the
    /// *transitions* agree, exhaustively. It says nothing about the actions,
    /// and the actions are the other half of the parser -- so this drives both
    /// spellings over whole programs and compares what they leave behind:
    /// every cell, the cursor, and every byte of the reply.
    ///
    /// Without it the switch to the graph would have made the transition
    /// differential vacuous -- a lookup checked against itself -- which is the
    /// stale `scan path: AVX2` failure this repo already records.
    #[test]
    #[cfg(feature = "rule-walk")]
    fn the_graph_agrees_with_the_rule_walk(ops in program()) {
        let b = bytes_of(&ops);
        for m in [OpMode::Computed, OpMode::Templated] {
            prop_assert_eq!(
                drive_bytewise(&b, m, false),
                drive_bytewise(&b, m, true)
            );
        }
    }

    /// **The bulk path must do what the per-cell path does.**
    ///
    /// `feed` brings a scanner, `fast_csi`, `fast_esc` and `put_ascii_run`;
    /// driving the same bytes one at a time brings none of them. Everything
    /// they add is an optimisation, so the two must be indistinguishable --
    /// and this repo has already been bitten once by `fast.rs` and
    /// `machine.rs` disagreeing past the sixteenth CSI parameter, through
    /// 20,000 passing cases of a property that could not build the shape.
    ///
    /// Found the moment it was written, which is why it is separate from
    /// `the_graph_agrees_with_the_rule_walk`: that one was comparing the
    /// parser *and* the plumbing, and so was answering neither question.
    #[test]
    #[cfg(feature = "rule-walk")]
    fn the_bulk_path_agrees_with_the_per_byte_path(ops in program()) {
        let b = bytes_of(&ops);
        for m in [OpMode::Computed, OpMode::Templated] {
            prop_assert_eq!(drive(&b, 4096, m), drive_bytewise(&b, m, false));
        }
    }

    fn generated_programs_are_chunk_invariant(ops in program(), a in 1usize..=64) {
        let b = bytes_of(&ops);
        for m in [OpMode::Computed, OpMode::Templated] {
            prop_assert_eq!(drive(&b, a, m), drive(&b, 4096, m));
        }
    }

    /// The aarch64 body and the portable path must agree on valid sequences.
    ///
    /// **The mirror of the AVX2 property, and it exists for the same reason
    /// that one does.** `Neon` differs from `Portable` in exactly one place --
    /// the lane mask comes out of `shrn`, in nibble encoding, where the
    /// portable form gets one bit a lane. Getting that wrong is a factor-of-
    /// four error in the returned offset that is invisible whenever the first
    /// match lands in lane zero, which on real text is most of the time.
    ///
    /// Unlike the AVX2 one this actually runs on a development machine here,
    /// so it is the first of the pair that is not taken on trust from CI.
    #[test]
    #[cfg(target_arch = "aarch64")]
    fn generated_programs_match_on_neon(ops in program(), chunk in 1usize..=128) {
        let b = bytes_of(&ops);
        let mut store = Storage::boxed();
        let mut t = Term::new(&mut store, OpMode::Computed);
        let mut i = 0;
        while i < b.len() {
            let n = chunk.min(READ_CHUNK).min(b.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let filled = slot.write(StreamByte::slice_from(&b[i..i + n]));
            t.feed_neon(base, filled.len());
            t.commit_read(filled.len());
            i += n;
        }
        prop_assert_eq!(t.viewport().to_vec(), drive(&b, chunk, OpMode::Computed).0);
    }

    /// The AVX2 body and the portable path must agree on valid sequences too.
    #[test]
    #[cfg(target_arch = "x86_64")]
    fn generated_programs_match_on_avx2(ops in program(), chunk in 1usize..=128) {
        let b = bytes_of(&ops);
        let mut store = Storage::boxed();
        let mut t = Term::new(&mut store, OpMode::Computed);
        let mut i = 0;
        while i < b.len() {
            let n = chunk.min(READ_CHUNK).min(b.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let filled = slot.write(StreamByte::slice_from(&b[i..i + n]));
            unsafe { t.feed_avx2(base, filled.len()) };
            t.commit_read(filled.len());
            i += n;
        }
        prop_assert_eq!(t.viewport().to_vec(), drive(&b, chunk, OpMode::Computed).0);
    }

    /// The cursor is always on the grid, whatever the program did to the
    /// margins, the origin mode or the autowrap flag.
    ///
    /// `Term::cursor()` is the reported position, not the internal one: with
    /// autowrap on, `cx` sits one past the last column as a deferred-wrap
    /// sentinel, and this property is what established that the sentinel was
    /// leaking out through CPR as a column that does not exist.
    #[test]
    fn the_cursor_never_leaves_the_grid(ops in program()) {
        let (_, (r, c), _) = drive(&bytes_of(&ops), 4096, OpMode::Computed);
        prop_assert!(r < 50, "row {} off the grid", r);
        prop_assert!(c < 200, "col {} off the grid", c);
    }

    /// Every reply is a well-formed control string, of the family its
    /// introducer promises.
    ///
    /// An application parses whatever arrives as the report it asked for, so a
    /// malformed reply is worse than no reply. baud now answers in three
    /// families — CSI for DA/DSR/DECRPM/mouse, DCS for DECRQSS and XTVERSION,
    /// OSC for a colour query — and each has its own shape and terminator.
    /// This walks the whole buffer, so a reply that ran past its terminator
    /// would be caught as the *next* one failing to start with ESC.
    #[test]
    fn replies_are_well_formed(ops in program()) {
        let (_, _, reply) = drive(&bytes_of(&ops), 4096, OpMode::Computed);
        prop_assert!(reply.len() <= 256);
        let mut i = 0;
        while i < reply.len() {
            // After `S8C1T` (`ESC SP G`) baud answers with the 8-bit C1
            // introducers -- `9B` for CSI, `90` for DCS, `9D` for OSC -- which
            // is what `docs/vt-coverage.md` says that row does. This assertion
            // used to require `ESC`, so it was a property about the *default*
            // encoding wearing the name of a property about replies; it only
            // ever passed because nothing generated could switch.
            //
            // `kind` is the introducer normalised to its 7-bit spelling, and
            // `body` is where the parameters start in either case.
            let (kind, body) = match reply[i] {
                0x1b => {
                    prop_assert!(i + 1 < reply.len(), "a lone ESC is not a reply");
                    (reply[i + 1], i + 2)
                }
                0x9b => (b'[', i + 1),
                0x90 => (b'P', i + 1),
                0x9d => (b']', i + 1),
                other => {
                    prop_assert!(false, "unknown reply introducer {}", other);
                    unreachable!()
                }
            };
            match kind {
                b'[' => {
                    let mut j = body;
                    // The DA identity carries `?`; the mouse report carries `<`.
                    if j < reply.len() && (reply[j] == b'?' || reply[j] == b'<') {
                        j += 1;
                    }
                    while j < reply.len()
                        && (StreamByte::from_stream(reply[j]).as_ascii_digit().is_some()
                            || reply[j] == b';')
                    {
                        j += 1;
                    }
                    // Intermediates: DECRPM ends `$ y`.
                    while j < reply.len()
                        && StreamByte::from_stream(reply[j]).is_csi_intermediate()
                    {
                        j += 1;
                    }
                    prop_assert!(j < reply.len(), "unterminated CSI reply");
                    prop_assert!(
                        FinalByte::from_stream(StreamByte::from_stream(reply[j])).is_some(),
                        "a CSI reply must end on a legal final byte"
                    );
                    i = j + 1;
                }
                // VT52's identity, `ESC / Z`. The one reply that is neither a
                // CSI nor a control string, because in VT52 mode there is no
                // CSI to answer with -- xterm's `CASE_DECID` emits `ESC`, `/`
                // and the id byte. This arm did not exist while nothing could
                // enter VT52 mode, so the property was about ANSI replies
                // under the name of a property about replies.
                b'/' => {
                    prop_assert!(body < reply.len(), "ESC / with no identity byte");
                    prop_assert_eq!(reply[body], b'Z', "baud identifies as a VT52");
                    i = body + 1;
                }
                // DCS and OSC are string families: consume to the terminator.
                b'P' | b']' => {
                    let bel_ok = kind == b']';
                    let mut j = body;
                    loop {
                        prop_assert!(j < reply.len(), "unterminated control string");
                        if bel_ok && reply[j] == 0x07 {
                            j += 1;
                            break;
                        }
                        // `9C` is ST's 8-bit spelling, used after S8C1T.
                        if reply[j] == 0x9c {
                            j += 1;
                            break;
                        }
                        if reply[j] == 0x1b {
                            prop_assert!(j + 1 < reply.len(), "ESC with no terminator");
                            prop_assert_eq!(
                                reply[j + 1], b'\\',
                                "only ESC-backslash terminates a string reply"
                            );
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                }
                other => prop_assert!(false, "unknown reply introducer {}", other),
            }
        }
    }

    // ── selective erase, algebraically ──────────────────────────────────────

    /// DECSEL erases a subset of what EL erases: every cell it touched, EL
    /// would have made blank the same way, and every cell EL left alone DECSEL
    /// left alone too.
    ///
    /// This is the invariant that fails if the two are aliases (DECSEL erasing
    /// protected cells) or if DECSEL erases something EL does not.
    #[test]
    fn decsel_erases_a_subset_of_el(ops in program(), extent in 0u8..3) {
        // Also through `grid_then`, so the baseline carries the same FLUSH the
        // two erase variants do.
        let before = grid_then(&ops, &[]);
        let sel = grid_then(&ops, &[Op::Decsel(extent)]);
        let plain = grid_then(&ops, &[Op::El(extent)]);
        for i in 0..before.len() {
            prop_assert!(
                sel[i] == before[i] || sel[i] == plain[i],
                "cell {} is neither kept nor erased-as-EL-would", i
            );
            if plain[i] == before[i] {
                prop_assert_eq!(sel[i], before[i], "cell {} outside EL's span moved", i);
            }
        }
    }

    /// The same, for the display.
    #[test]
    fn decsed_erases_a_subset_of_ed(ops in program(), extent in 0u8..3) {
        // Also through `grid_then`, so the baseline carries the same FLUSH the
        // two erase variants do.
        let before = grid_then(&ops, &[]);
        let sel = grid_then(&ops, &[Op::Decsed(extent)]);
        let plain = grid_then(&ops, &[Op::Ed(extent)]);
        for i in 0..before.len() {
            prop_assert!(sel[i] == before[i] || sel[i] == plain[i]);
            if plain[i] == before[i] {
                prop_assert_eq!(sel[i], before[i]);
            }
        }
    }

    /// With no DECSCA anywhere in the program, the selective erases are exactly
    /// the plain ones. Anything else means protection leaked in from somewhere
    /// that never set it.
    #[test]
    fn without_decsca_selective_equals_plain(ops in program(), extent in 0u8..3) {
        let ops: Vec<Op> = ops
            .into_iter()
            .filter(|o| !matches!(o, Op::Decsca(_)))
            .collect();
        prop_assert_eq!(
            grid_then(&ops, &[Op::Decsel(extent)]),
            grid_then(&ops, &[Op::El(extent)])
        );
        prop_assert_eq!(
            grid_then(&ops, &[Op::Decsed(extent)]),
            grid_then(&ops, &[Op::Ed(extent)])
        );
    }

    /// Erasing is idempotent: none of the four erases moves the cursor or
    /// changes anything on a second application.
    ///
    /// Deliberately weaker than it looks -- it does *not* catch a blank that
    /// inherits the protection, because the second erase finds those cells
    /// already blank. `a_cleared_screen_is_fully_erasable` is the one that
    /// catches that. This one catches an erase whose span depends on state the
    /// erase itself changed.
    #[test]
    fn erasing_twice_is_erasing_once(ops in program(), extent in 0u8..3) {
        for e in [Op::El(extent), Op::Ed(extent), Op::Decsel(extent), Op::Decsed(extent)] {
            prop_assert_eq!(grid_then(&ops, &[e]), grid_then(&ops, &[e, e]));
        }
    }

    /// Nothing on a cleared screen is protected.
    ///
    /// After `ED 2` the display holds blanks and no characters, and
    /// `[[source: vt510-decsca]]` scopes protection to "characters" -- so from
    /// there a selective erase must be indistinguishable from a plain one. The
    /// rendition is changed in between so the two erases have something visible
    /// to disagree about: if the blanks `ED 2` wrote had inherited the
    /// protection from a `DECSCA 1` still in force, `DECSED` would refuse to
    /// repaint them and the backgrounds would differ.
    #[test]
    fn a_cleared_screen_is_fully_erasable(ops in program()) {
        prop_assert_eq!(
            grid_then(&ops, &[Op::Ed(2), Op::Sgr(41), Op::Decsed(2)]),
            grid_then(&ops, &[Op::Ed(2), Op::Sgr(41), Op::Ed(2)])
        );
    }

    /// Anything done on the alternate screen leaves the primary screen exactly
    /// as it was.
    ///
    /// The whole reason the alt screen is worth a second 10 MiB buffer. This
    /// runs an arbitrary program, snapshots the primary screen, then runs a
    /// second arbitrary program entirely inside an alt-screen switch and checks
    /// the primary came back byte-identical.
    #[test]
    fn the_alt_screen_never_leaks_into_the_primary(
        before in program(),
        inside in program(),
    ) {
        // The prefix must leave the terminal on the *primary* screen, or the
        // baseline is a snapshot of the alt screen and the comparison is
        // meaningless. Filtering the switch out of it is the simplest way to
        // guarantee that without constraining what else it may do.
        let before: Vec<Op> = before
            .into_iter()
            .filter(|o| !matches!(o, Op::Decset(1049) | Op::Decrst(1049) | Op::Ris))
            .collect();
        let baseline = grid_then(&before, &[]);
        let mut v = before.clone();
        v.push(FLUSH);
        v.push(Op::Decset(1049));
        // A `?1049 l` *inside* is not a leak -- it is the program leaving the
        // alt screen, after which it is entitled to write to the primary one.
        // Filtering it is what makes this a test of isolation rather than of
        // the generator. `?1049 h` stays, so re-entering is still exercised.
        //
        // `RIS` goes with it, and for the same reason rather than a weaker
        // one: `ESC c` is an explicit instruction to discard everything, and
        // xterm's `ReallyReset` calls `FromAlternate` on its `if (full)` path.
        // Leaving the alt screen is what RIS is *for*. That is precisely the
        // distinction `DECSTR` failed -- it took the same action without the
        // same licence, and this property is what caught it, so the filter has
        // to name RIS specifically and not "any reset".
        v.extend(
            inside
                .iter()
                .copied()
                .filter(|o| !matches!(o, Op::Decrst(1049) | Op::Ris)),
        );
        v.push(Op::Decrst(1049));
        prop_assert_eq!(grid(&v), baseline);
    }

    /// DECSCA on its own changes nothing on screen. It arms the erases; it does
    /// not paint.
    #[test]
    fn decsca_alone_is_invisible(ops in program(), p in 0u8..3) {
        prop_assert_eq!(grid_then(&ops, &[]), grid_then(&ops, &[Op::Decsca(p)]));
    }
}

/// **The generator emits what its variants claim.**
///
/// This file's whole lesson is that a property is only as good as the shapes
/// its generator reaches, and an op that silently emits the wrong bytes is
/// indistinguishable from one whose shape is simply never buggy. The same
/// failure as a search probe that cannot match: it looks exactly like a fact.
///
/// So each axis added after the parameter-overflow bug is checked here against
/// the bytes it is supposed to produce, deterministically.
#[test]
fn the_generator_reaches_the_shapes_it_claims() {
    let many = bytes_of(&[Op::SgrMany(40, 0)]);
    assert_eq!(
        many.iter().filter(|b| **b == b';').count(),
        39,
        "SgrMany(40) must emit forty parameters, crossing CAPACITY {}",
        ParamIdx::CAPACITY
    );

    let holes = bytes_of(&[Op::SgrHoles(9, 0)]);
    assert!(
        holes.windows(2).any(|w| w == b";;"),
        "SgrHoles must actually leave a parameter empty, got {:?}",
        core::str::from_utf8(&holes).unwrap()
    );

    let huge = bytes_of(&[Op::CupHuge(3)]);
    assert!(
        core::str::from_utf8(&huge).unwrap().contains("4294967296"),
        "CupHuge must exceed Param's u16 so the saturation is taken"
    );

    let c0 = bytes_of(&[Op::CsiWithC0(0x07, 2)]);
    let esc = c0.iter().position(|b| *b == 0x1b).unwrap();
    let fin = c0.iter().position(|b| *b == b'm').unwrap();
    assert!(
        c0[esc..fin].contains(&0x07),
        "CsiWithC0 must splice the C0 *inside* the sequence, got {c0:?}"
    );

    // Control strings: the introducer must be present and the payload must sit
    // *inside* it, before the ST.
    for (kind, name) in [(b'X', "SOS"), (b'^', "PM"), (b'_', "APC"), (b'P', "DCS")] {
        let b = bytes_of(&[Op::ControlString(kind, 0)]);
        assert_eq!(b[0], 0x1b, "{name} starts with ESC");
        assert_eq!(b[1], kind, "{name} uses its own introducer");
        let st = b.windows(2).position(|w| w == b"\x1b\\").expect("ST");
        assert!(
            b[2..st].starts_with(b"Gpayload"),
            "{name} payload must precede the terminator"
        );
    }

    // And the ideogram aspects are reachable at all, which they were not
    // before: the SGR table stopped at 107.
    assert!(SGR.contains(&64), "SGR 64 must be generatable");
    assert!(SGR.contains(&23), "and the two-bit clears too");
}

// ---------------------------------------------------------------------------
// Generator reach, enumerated rather than imagined.
// ---------------------------------------------------------------------------

/// A CSI form as it appears in the byte stream: optional private marker,
/// optional intermediate, final byte. Parameters are not part of the identity.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
struct Form {
    marker: Option<u8>,
    inter: Option<u8>,
    final_byte: u8,
}

/// Every CSI form present in a byte stream.
fn forms_in(bytes: &[u8], out: &mut std::collections::BTreeSet<Form>) {
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] != 0x1b || bytes[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let mut j = i + 2;
        let mut marker = None;
        if j < bytes.len() && matches!(bytes[j], b'?' | b'>' | b'<' | b'=') {
            marker = Some(bytes[j]);
            j += 1;
        }
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';' || bytes[j] == b':')
        {
            j += 1;
        }
        let mut inter = None;
        while j < bytes.len() && (0x20..=0x2f).contains(&bytes[j]) {
            inter = Some(bytes[j]);
            j += 1;
        }
        if j < bytes.len() && (0x40..=0x7e).contains(&bytes[j]) {
            out.insert(Form {
                marker,
                inter,
                final_byte: bytes[j],
            });
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

/// Draw a large deterministic corpus from the same strategy the properties use.
fn generated_corpus() -> std::collections::BTreeSet<Form> {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config {
        cases: 1,
        ..Config::default()
    });
    let mut forms = std::collections::BTreeSet::new();
    let strat = program();
    for _ in 0..4000 {
        let tree = strat.new_tree(&mut runner).expect("generate");
        let ops = tree.current();
        forms_in(&bytes_of(&ops), &mut forms);
    }
    forms
}

/// The CSI forms `docs/vt-coverage.md` marks **done**.
fn implemented_forms() -> std::collections::BTreeSet<Form> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("vt-coverage.md");
    let mut out = std::collections::BTreeSet::new();
    for line in src.lines() {
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.split('|').collect();
        if cells.len() < 3 || !cells[2..].iter().any(|c| c.contains("done")) {
            continue;
        }
        // A backtick final byte (HPA) needs a double-backtick code span, so
        // strip the delimiters rather than every backtick -- `trim_matches`
        // would eat the operation's own final byte.
        let cell = cells[1].trim();
        let seq = match cell.strip_prefix("``").and_then(|c| c.strip_suffix("``")) {
            Some(inner) => inner.trim(),
            None => cell.trim_matches('`'),
        };
        let Some(rest) = seq.strip_prefix("CSI ") else {
            continue;
        };
        let toks: Vec<&str> = rest.split_whitespace().collect();
        let Some((last, head)) = toks.split_last() else {
            continue;
        };
        let fb = last.as_bytes();
        if fb.len() != 1 || !(0x40..=0x7e).contains(&fb[0]) {
            continue;
        }
        let (mut marker, mut inter) = (None, None);
        let mut ok = true;
        for (n, t) in head.iter().enumerate() {
            let b = t.as_bytes();
            if *t == "SP" {
                inter = Some(0x20);
            } else if t.starts_with('P') || *t == ";" || t.chars().all(|c| c.is_ascii_digit()) {
                continue;
            } else if b.len() == 1 && n == 0 && matches!(b[0], b'?' | b'>' | b'<' | b'=') {
                marker = Some(b[0]);
            } else if b.len() == 1 && (0x20..=0x2f).contains(&b[0]) {
                inter = Some(b[0]);
            } else {
                ok = false;
                break;
            }
        }
        if ok {
            out.insert(Form {
                marker,
                inter,
                final_byte: fb[0],
            });
        }
    }
    out
}

/// **Every CSI form the coverage table marks `done` is reachable by the
/// generator.**
///
/// The parameter-overflow bug was invisible to a property suite running 20,000
/// cases because `Op::Sgr` emitted one parameter and nothing built a longer
/// list. The fix for *that* bug was one op; the fix for the class is this
/// test, which enumerates the gap instead of leaving it to whoever thinks to
/// look.
///
/// It reads `docs/vt-coverage.md` and a corpus drawn from the same strategy
/// the properties use, and requires every claimed form to appear in the
/// corpus. When first run it found eleven: CNL, CPL, CHT, CBT, HPA, HPR, VPR,
/// TBC, DECSTR, and -- most worth having -- SM and RM in the *unprefixed*
/// namespace, because `Decset`/`Decrst` only ever emitted the `?` form. SM/RM
/// is an operation this project has already had sitting parsed-and-dropped.
///
/// It also found a documentation bug: HPA's row was written `` `CSI \`` ``,
/// and CommonMark does not process backslash escapes inside a code span, so it
/// rendered as `CSI \` rather than naming the backtick final byte at all.
///
/// This is the same direction as `spec_inventory.rs` -- the document checks
/// the code, rather than the code being described by the document. A row that
/// says `done` and cannot be generated is a row nothing random will ever
/// reach.
#[test]
fn every_done_form_is_reachable_by_the_generator() {
    let reached = generated_corpus();
    let claimed = implemented_forms();
    assert!(
        claimed.len() > 30,
        "parsed only {} done forms from vt-coverage.md -- the table's shape \
         changed and this check is now measuring nothing",
        claimed.len()
    );

    let missed: Vec<String> = claimed
        .difference(&reached)
        .map(|f| {
            format!(
                "CSI {}{}{}",
                f.marker
                    .map(|b| (b as char).to_string())
                    .unwrap_or_default(),
                f.inter.map(|b| (b as char).to_string()).unwrap_or_default(),
                f.final_byte as char
            )
        })
        .collect();
    assert!(
        missed.is_empty(),
        "{} form(s) marked done in docs/vt-coverage.md that no generated \
         program emits, so no property can reach them:\n  {}",
        missed.len(),
        missed.join("\n  ")
    );
}

/// An `ESC` form: optional intermediate, then the final byte.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
struct Esc {
    inter: Option<u8>,
    final_byte: u8,
}

/// Every `ESC` form in a byte stream, skipping `ESC [`, `ESC ]` and `ESC P`,
/// which introduce CSI, OSC and DCS rather than being forms themselves.
fn escapes_in(bytes: &[u8], out: &mut std::collections::BTreeSet<Esc>) {
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] != 0x1b {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if matches!(bytes[j], b'[' | b']' | b'P' | b'X' | b'^' | b'_') {
            i += 2;
            continue;
        }
        let mut inter = None;
        if (0x20..=0x2f).contains(&bytes[j]) {
            inter = Some(bytes[j]);
            j += 1;
        }
        if j < bytes.len() && (0x30..=0x7e).contains(&bytes[j]) {
            out.insert(Esc {
                inter,
                final_byte: bytes[j],
            });
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

/// The `ESC` forms `docs/vt-coverage.md` marks **done**, restricted to rows
/// that name exactly one form.
///
/// The charset-designation rows list a whole family on one line
/// (`ESC ( C 5 H 7 K Q 9 R f Y Z 4 = ...`), which is a set of finals for one
/// intermediate rather than a single form; `every_designator_is_reachable`
/// covers those instead.
fn implemented_escapes() -> std::collections::BTreeSet<Esc> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("vt-coverage.md");
    let mut out = std::collections::BTreeSet::new();
    for line in src.lines() {
        if !line.starts_with('|') || !line.contains("done") {
            continue;
        }
        for span in line.split('`').skip(1).step_by(2) {
            let Some(rest) = span.trim().strip_prefix("ESC ") else {
                continue;
            };
            let toks: Vec<&str> = rest.split_whitespace().collect();
            let (inter, fin) = match toks.len() {
                1 => (None, toks[0]),
                2 if toks[0] == "SP" => (Some(0x20u8), toks[1]),
                2 if toks[0].len() == 1
                    && (0x20..=0x2f).contains(&toks[0].as_bytes()[0])
                    && toks[1].len() == 1 =>
                {
                    (Some(toks[0].as_bytes()[0]), toks[1])
                }
                _ => continue,
            };
            let fb = fin.as_bytes();
            // `ESC P`, `ESC [`, `ESC ]`, `ESC X`, `ESC ^`, `ESC _` are the
            // 7-bit spellings of the C1 *introducers*. They begin a CSI, OSC
            // or control string rather than being forms in their own right,
            // and `escapes_in` skips them for the same reason.
            let introducer =
                inter.is_none() && matches!(fb[0], b'P' | b'[' | b']' | b'X' | b'^' | b'_');
            if fb.len() == 1 && (0x30..=0x7e).contains(&fb[0]) && !introducer {
                out.insert(Esc {
                    inter,
                    final_byte: fb[0],
                });
            }
        }
    }
    out
}

/// Every `OSC Ps` in a byte stream.
fn oscs_in(bytes: &[u8], out: &mut std::collections::BTreeSet<u16>) {
    let mut i = 0;
    while i + 1 < bytes.len() {
        let body = if bytes[i] == 0x1b && bytes[i + 1] == b']' {
            i + 2
        } else if bytes[i] == 0x9d {
            i + 1
        } else {
            i += 1;
            continue;
        };
        let mut j = body;
        let mut ps: u32 = 0;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            ps = ps * 10 + u32::from(bytes[j] - b'0');
            j += 1;
        }
        if j > body && ps <= u32::from(u16::MAX) {
            out.insert(ps as u16);
        }
        i = body;
    }
}

/// The `OSC Ps` values `docs/vt-coverage.md` marks **done**.
fn implemented_oscs() -> std::collections::BTreeSet<u16> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("vt-coverage.md");
    let mut out = std::collections::BTreeSet::new();
    for line in src.lines() {
        if !line.starts_with("| `OSC ") || !line.contains("done") {
            continue;
        }
        let Some(span) = line.split('`').nth(1) else {
            continue;
        };
        let Some(rest) = span.trim().strip_prefix("OSC ") else {
            continue;
        };
        // `OSC 0/1/2` names three commands on one row.
        for part in rest.split('/') {
            if let Ok(n) = part.trim().parse::<u16>() {
                out.insert(n);
            }
        }
    }
    out
}

/// **Every `ESC` form and `OSC Ps` the table marks `done` is reachable.**
///
/// The companion to `every_done_form_is_reachable_by_the_generator`, which
/// covers CSI. Splitting the parser's input surface by introducer and checking
/// each is what turns "the generator might still miss something" into a list.
///
/// The ESC pass found nine unreachable when first run -- DECKPAM, DECKPNM,
/// HTS, RIS, S7C1T, S8C1T, DECALN and the two `ESC %` encoding selectors --
/// and adding them broke two properties immediately. Neither was a bug in
/// baud: `replies_are_well_formed` required an `ESC` introducer and so was a
/// property about the *default* encoding wearing the name of a property about
/// replies, and the alt-screen isolation property had to be told that RIS
/// leaving the alt screen is licensed where DECSTR doing it was not.
///
/// **DCS is absent by construction, not by omission.** Both `DCS` rows are
/// `out` -- Sixel and DECUDK -- so there is nothing to enumerate. DECRQSS is
/// the one DCS form baud answers and it is reached via `Op::Decrqss`; it is
/// recorded in the parser-state table rather than as a `DCS` row, which is why
/// it does not appear here.
#[test]
fn every_done_escape_and_osc_is_reachable_by_the_generator() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config {
        cases: 1,
        ..Config::default()
    });
    let strat = program();
    let mut escapes = std::collections::BTreeSet::new();
    let mut oscs = std::collections::BTreeSet::new();
    for _ in 0..4000 {
        let ops = strat.new_tree(&mut runner).expect("gen").current();
        let b = bytes_of(&ops);
        escapes_in(&b, &mut escapes);
        oscs_in(&b, &mut oscs);
    }

    let claimed_esc = implemented_escapes();
    assert!(
        claimed_esc.len() > 12,
        "parsed only {} done ESC forms -- the table's shape changed and this \
         check is measuring nothing",
        claimed_esc.len()
    );
    let missed: Vec<String> = claimed_esc
        .difference(&escapes)
        .map(|f| {
            format!(
                "ESC {}{}",
                f.inter
                    .map(|b| format!("{} ", b as char))
                    .unwrap_or_default(),
                f.final_byte as char
            )
        })
        .collect();
    assert!(
        missed.is_empty(),
        "{} ESC form(s) marked done that no generated program emits:\n  {}",
        missed.len(),
        missed.join("\n  ")
    );

    let claimed_osc = implemented_oscs();
    assert!(
        claimed_osc.len() >= 5,
        "parsed only {} done OSC commands",
        claimed_osc.len()
    );
    let missed: Vec<String> = claimed_osc
        .difference(&oscs)
        .map(|n| format!("OSC {n}"))
        .collect();
    assert!(
        missed.is_empty(),
        "{} OSC command(s) marked done that no generated program emits:\n  {}",
        missed.len(),
        missed.join("\n  ")
    );
}

/// **Every final the generator sends with a parameter, it also sends bare and
/// saturating.**
///
/// Reachability was the first rung: `every_done_form_is_reachable_by_the_
/// generator` proves each operation is *emitted*. It says nothing about the
/// values, and for a long time the answer was that every numeric op always
/// wrote a number — of the thirty-one finals the generator produced, only `c`
/// was ever sent bare.
///
/// A defaulted parameter is a distinct code path per operation, and the
/// defaults are not uniform: CUP is 1;1, SU is 1, SGR is 0, DECSTBM is the
/// whole screen. This project has already had an off-by-one in exactly that
/// arithmetic (CPR), so "the operation was dispatched" is not the same as "the
/// operation was dispatched with the parameter it would get in the wild".
///
/// The saturating case is the other end of the same axis: `Param::push`
/// saturates at 65535, and until `Op::Extreme` that ceiling was only ever
/// reached through CUP.
///
/// This check derives both sets from the corpus rather than from
/// `PARAMETERISED`, so adding a parameterised op without listing it fails
/// here instead of silently narrowing what the properties explore.
#[test]
fn every_parameterised_final_is_also_sent_bare_and_saturating() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config {
        cases: 1,
        ..Config::default()
    });
    let strat = program();

    let (mut with_param, mut bare, mut big) = (
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::new(),
    );
    for _ in 0..4000 {
        let b = bytes_of(&strat.new_tree(&mut runner).expect("gen").current());
        let mut i = 0;
        while i + 1 < b.len() {
            if b[i] != 0x1b || b[i + 1] != b'[' {
                i += 1;
                continue;
            }
            let mut j = i + 2;
            if j < b.len() && matches!(b[j], b'?' | b'>' | b'<' | b'=') {
                j += 1;
            }
            let start = j;
            let mut digits = 0;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b';' || b[j] == b':') {
                if b[j].is_ascii_digit() {
                    digits += 1;
                } else {
                    digits = 0;
                }
                j += 1;
            }
            let run = digits;
            let had = j > start;
            let mut inter = 0u8;
            while j < b.len() && (0x20..=0x2f).contains(&b[j]) {
                inter = b[j];
                j += 1;
            }
            if j < b.len() && (0x40..=0x7e).contains(&b[j]) {
                // Only the plain namespace: `?25h` is a mode, and a bare
                // `CSI ? h` is not a spelling any application sends.
                if b[i + 2] != b'?' && b[i + 2] != b'>' && b[i + 2] != b'<' && b[i + 2] != b'=' {
                    let key = (inter, b[j]);
                    if had {
                        with_param.insert(key);
                        if run >= 5 {
                            big.insert(key);
                        }
                    } else {
                        bare.insert(key);
                    }
                }
                i = j + 1;
            } else {
                i += 1;
            }
        }
    }

    assert!(
        with_param.len() >= 25,
        "only {} parameterised finals seen -- the generator shrank and this \
         check is measuring nothing",
        with_param.len()
    );

    let show = |(i, f): &(u8, u8)| match i {
        0 => format!("CSI {}", *f as char),
        _ => format!("CSI {} {}", *i as char, *f as char),
    };
    let no_default: Vec<String> = with_param.difference(&bare).map(show).collect();
    assert!(
        no_default.is_empty(),
        "{} final(s) only ever sent with a parameter, so no property exercises \
         their default: {:?}",
        no_default.len(),
        no_default
    );

    let no_big: Vec<String> = with_param.difference(&big).map(show).collect();
    assert!(
        no_big.is_empty(),
        "{} final(s) never sent a saturating value, so `Param::push`'s ceiling \
         is unexercised for them: {:?}",
        no_big.len(),
        no_big
    );
}

/// **Every designator baud implements is reached, in every G-slot.**
///
/// The charset rows are excluded from the `ESC` reach check because each names
/// a whole family on one line (`ESC ( C 5 H 7 K Q 9 R f Y Z 4 = ...`), so this
/// is their equivalent. It derives the expected set from `Charset::from_final`
/// rather than from `DESIGNATORS`, so adding a designator to the parser and
/// not to the generator fails here instead of silently narrowing the corpus.
///
/// This test was *cited before it was written*: a doc comment above claimed it
/// covered the charset rows while no such function existed.
/// `every_test_named_in_the_docs_exists` did not catch it because that scanner
/// reads markdown, and this was a Rust doc comment — which is the same defect
/// as a `done` row with no test, one file type over.
#[test]
fn every_designator_is_reachable() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, TestRunner};

    // What the parser implements: every byte `from_final` maps off `Other`.
    let implemented: std::collections::BTreeSet<u8> = (0x30u8..=0x7e)
        .filter(|b| {
            baud_domain::Charset::from_final(StreamByte::from_stream(*b))
                != baud_domain::Charset::Other
        })
        .collect();
    assert!(
        implemented.len() >= 20,
        "only {} designators map off Other -- the mapping changed shape",
        implemented.len()
    );

    let mut runner = TestRunner::new(Config {
        cases: 1,
        ..Config::default()
    });
    let strat = program();
    let mut seen: std::collections::BTreeSet<(u8, u8)> = std::collections::BTreeSet::new();
    for _ in 0..4000 {
        let b = bytes_of(&strat.new_tree(&mut runner).expect("gen").current());
        let mut i = 0;
        while i + 2 < b.len() {
            if b[i] == 0x1b && matches!(b[i + 1], b'(' | b')' | b'*' | b'+') {
                seen.insert((b[i + 1], b[i + 2]));
                i += 3;
            } else {
                i += 1;
            }
        }
    }

    let slots = [b'(', b')', b'*', b'+'];
    let mut missing = Vec::new();
    for g in slots {
        for d in &implemented {
            if !seen.contains(&(g, *d)) {
                missing.push(format!("ESC {} {}", g as char, *d as char));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "{} designation(s) no generated program makes, so nothing random \
         exercises their translation:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

proptest! {
    /// **Resizing between reads is sound, whatever the stream did first.**
    ///
    /// `resize` is a public entry point that no generated program could reach,
    /// because the generator emits bytes and this is an API call. That gap hid
    /// a crash: `RowRot` is bounded by the row count at the time it was
    /// advanced, so a shrink could leave it past the new one, and
    /// `viewport`'s `rotate_left` then panicked with `mid <= self.len()` —
    /// 41 rows of rotation against a ten-row slice. Scroll a screenful, drag
    /// the window smaller, read the grid.
    ///
    /// The stream is fed in two halves with the resize between them, because
    /// resizing a terminal that is *mid-sequence* is the case an application
    /// cannot control: `SIGWINCH` does not wait for a CSI to finish.
    #[test]
    fn resize_between_reads_is_sound(
        before in program(),
        after in program(),
        rows in 1u16..=130,
        cols in 1u16..=260,
    ) {
        let mut store = Storage::boxed();
        let mut t = Term::new(&mut store, OpMode::Computed);

        for (k, part) in [bytes_of(&before), bytes_of(&after)].iter().enumerate() {
            if k == 1 {
                let _ = t.resize(&WinSize { ws_row: rows, ws_col: cols });
            }
            let mut i = 0;
            while i < part.len() {
                let n = READ_CHUNK.min(part.len() - i);
                let mut slot = t.read_slot();
                let base = slot.base();
                let f = slot.write(StreamByte::slice_from(&part[i..i + n]));
                t.feed(base, f.len());
                t.commit_read(f.len());
                i += n;
            }
        }

        // Each of these read the rotation, and each panicked before the fix.
        let (r, c) = t.cursor();
        let vp_len = t.viewport().len();
        prop_assert_eq!(vp_len % 2048, 0, "the viewport is whole rows");
        prop_assert!(r.index() < 130.min(vp_len / 2048).max(1), "cursor row in the resized grid");
        prop_assert!(c.index() <= 260, "cursor column in the resized grid");
    }
}

/// The case `decsed_erases_a_subset_of_ed` shrank to, pinned as a plain test.
///
/// `CSI 1 J` erases from the start of the display to the cursor; `CSI ? 1 J`
/// is the same span but spares protected cells. So DECSED must never touch a
/// cell ED leaves alone. Found by the property at 20,000 cases with no DECSCA
/// anywhere in the program, which means the protection it is respecting came
/// from somewhere that never set it.
#[test]
fn decsed_matches_ed_when_nothing_was_protected() {
    let ops = [
        Op::Utf8(6),
        Op::Rep(5),
        Op::Cht(0),
        Op::Rep(0),
        Op::Nel,
        FLUSH,
    ];
    let before = grid_then(&ops, &[]);
    let sel = grid_then(&ops, &[Op::Decsed(1)]);
    let plain = grid_then(&ops, &[Op::Ed(1)]);
    let mut bad = Vec::new();
    for i in 0..before.len() {
        let ok = (sel[i] == before[i] || sel[i] == plain[i])
            && (plain[i] != before[i] || sel[i] == before[i]);
        if !ok {
            bad.push((
                i,
                (
                    before[i].content == plain[i].content,
                    before[i].style,
                    before[i].flags.bits(),
                ),
                (
                    plain[i].content == sel[i].content,
                    plain[i].style,
                    plain[i].flags.bits(),
                ),
                (sel[i].style, sel[i].flags.bits()),
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "cells where DECSED and ED disagree with no DECSCA in the program \
         (index, [before: sameContent, style, flags], [ed: ...], [decsed: style, flags]): {:?}",
        &bad[..bad.len().min(8)]
    );
}
