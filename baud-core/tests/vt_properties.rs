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

use baud_core::*;
use proptest::prelude::*;

// ── a generator for well-formed sequences ───────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum Op {
    /// `n` copies of a printable character.
    Text(u8, u8),
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
}

/// SGR parameters baud implements, so a generated program exercises styles
/// rather than the "unknown attribute" break.
const SGR: [u8; 20] = [
    0, 1, 2, 3, 4, 5, 7, 8, 9, 22, 24, 27, 30, 39, 41, 49, 90, 97, 100, 107,
];

/// Private modes baud acts on, plus two it deliberately ignores -- an ignored
/// mode must stay inert, and that is worth generating too.
const MODES: [u16; 6] = [6, 7, 25, 1000, 1049, 2004];

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0x21u8..0x7f, 1u8..40).prop_map(|(c, n)| Op::Text(c, n)),
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
            Op::Text(c, k) => {
                for _ in 0..k {
                    out.push(c);
                }
                continue;
            }
            Op::Cup(r, c) => write!(n, "\x1b[{r};{c}H").unwrap(),
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
        }
        out.extend_from_slice(n.as_bytes());
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

fn grid(ops: &[Op]) -> Vec<PackedCell> {
    drive(&bytes_of(ops), 4096, OpMode::Computed).0
}

/// Run `ops`, then `extra`, and return the viewport.
fn grid_then(ops: &[Op], extra: &[Op]) -> Vec<PackedCell> {
    let mut v = ops.to_vec();
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
    #[test]
    fn generated_programs_are_chunk_invariant(ops in program(), a in 1usize..=64) {
        let b = bytes_of(&ops);
        for m in [OpMode::Runs, OpMode::Computed] {
            prop_assert_eq!(drive(&b, a, m), drive(&b, 4096, m));
        }
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
    #[test]
    fn the_cursor_never_leaves_the_grid(ops in program()) {
        let (_, (r, c), _) = drive(&bytes_of(&ops), 4096, OpMode::Computed);
        prop_assert!(r < 50, "row {} off the grid", r);
        prop_assert!(c < 200, "col {} off the grid", c);
    }

    /// Every reply is a well-formed CSI sequence, and the buffer never grows
    /// past its bound.
    ///
    /// An application parses whatever arrives as the report it asked for, so a
    /// malformed reply is worse than no reply. This checks the *shape* of what
    /// baud sends without re-encoding the reports, which would just restate the
    /// implementation.
    #[test]
    fn replies_are_well_formed(ops in program()) {
        let (_, _, reply) = drive(&bytes_of(&ops), 4096, OpMode::Computed);
        prop_assert!(reply.len() <= 256);
        let mut i = 0;
        while i < reply.len() {
            prop_assert_eq!(reply[i], 0x1b, "a reply must start with ESC");
            prop_assert!(i + 1 < reply.len(), "a lone ESC is not a reply");
            prop_assert_eq!(reply[i + 1], b'[', "CSI, not some other introducer");
            let mut j = i + 2;
            // The one private reply baud sends is the DA identity, `CSI ? 6 c`.
            if j < reply.len() && reply[j] == b'?' {
                j += 1;
            }
            // Parameter string: baud only ever emits digits and separators, so
            // this is stricter than ECMA-48's `0x30..=0x3f` on purpose.
            while j < reply.len()
                && (StreamByte::from_stream(reply[j]).as_ascii_digit().is_some()
                    || reply[j] == b';')
            {
                j += 1;
            }
            prop_assert!(j < reply.len(), "unterminated reply");
            prop_assert!(
                FinalByte::from_stream(StreamByte::from_stream(reply[j])).is_some(),
                "a reply must end on a legal final byte"
            );
            i = j + 1;
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
        let before = grid(&ops);
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
        let before = grid(&ops);
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

    /// DECSCA on its own changes nothing on screen. It arms the erases; it does
    /// not paint.
    #[test]
    fn decsca_alone_is_invisible(ops in program(), p in 0u8..3) {
        prop_assert_eq!(grid(&ops), grid_then(&ops, &[Op::Decsca(p)]));
    }
}
