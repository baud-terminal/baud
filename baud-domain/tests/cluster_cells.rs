//! How much of a cluster one cell holds ([`ClusterCells`]): the extended
//! machine, parted where a legacy variant breaks, against the variants'
//! own rules.

use baud_domain::{
    AmbiguousWidth, Classified, ClusterBreak, ClusterCells, ClusterRules, Codepoint, GraphemeClass,
    Lookback, UnicodeWidths, Width, WidthPolicy,
};
use std::collections::{HashSet, VecDeque};

const CELLS: [ClusterCells; 3] = [
    ClusterCells::Legacy,
    ClusterCells::LegacyMarksJoined,
    ClusterCells::Extended,
];

/// Every input the machine takes: four bits of class, four of lookback.
fn inputs() -> Vec<(GraphemeClass, Lookback)> {
    (0u8..14)
        .flat_map(|c| {
            (0u8..16).map(move |l| {
                (
                    GraphemeClass::FROM_INDEX[usize::from(c)],
                    Lookback::from_bits(l),
                )
            })
        })
        .collect()
}

/// **The parted extended machine is each variant's own machine**, over
/// every pair of states the two reach together: from the start of text and
/// from after a letter, at every input, the variant breaks exactly where the
/// extended machine breaks or parts, and a part leaves the machine where a
/// boundary would. So the core, which runs one machine and parts it, counts
/// the columns `[[source: uax29]]`'s legacy variant gives -- "the same rules
/// ... except the rules GB9a, GB9b, and GB9c" -- and, with spacing marks
/// joined, that variant with GB9a kept.
#[test]
fn the_legacy_variants_are_the_extended_machine_parted() {
    let ins = inputs();
    for cells in CELLS {
        let starts = [
            (ClusterBreak::new(), ClusterRules::new()),
            (
                ClusterBreak::after_ascii_run(),
                ClusterRules::after_ascii_run(),
            ),
        ];
        let mut seen: HashSet<(ClusterBreak, u16)> =
            starts.iter().map(|(e, r)| (*e, r.to_bits())).collect();
        let mut queue: VecDeque<(ClusterBreak, ClusterRules)> = starts.into_iter().collect();
        let mut steps = 0usize;
        while let Some((e, r)) = queue.pop_front() {
            for &(cl, lb) in &ins {
                let before = e;
                let mut e2 = e;
                let joins = !e2.is_break_classified(cl, lb);
                let parts = joins && before.parts(cells, cl, lb);
                if parts {
                    e2 = ClusterBreak::restart(cl, lb);
                }
                let mut r2 = r;
                let brk = r2.is_break_in(cells, cl, lb);
                assert_eq!(
                    brk,
                    !joins || parts,
                    "{cells:?}: {cl:?} {lb:?} from {:?}",
                    ClusterRules::from_bits(r.to_bits())
                );
                steps += 1;
                if seen.insert((e2, r2.to_bits())) {
                    queue.push_back((e2, r2));
                }
            }
        }
        assert!(steps > 20_000, "{cells:?}: only {steps} steps");
    }
}

/// **Under the extended variant nothing parts**, and its own machine is the
/// extended rules: `is_break_in` with every rule kept is `is_break_by_rules`.
#[test]
fn the_extended_variant_parts_nothing() {
    let ins = inputs();
    let mut queue = vec![ClusterRules::new()];
    let mut seen = HashSet::from([ClusterRules::new().to_bits()]);
    while let Some(r) = queue.pop() {
        for &(cl, lb) in &ins {
            let (mut a, mut b) = (r, r);
            assert_eq!(
                a.is_break_in(ClusterCells::Extended, cl, lb),
                b.is_break_by_rules(cl, lb)
            );
            assert_eq!(a.to_bits(), b.to_bits());
            if seen.insert(a.to_bits()) {
                queue.push(a);
            }
        }
    }
}

fn machine_after(text: &[u32]) -> ClusterBreak {
    let mut m = ClusterBreak::new();
    for &c in text {
        let cl = Classified::of(Codepoint::new(c).unwrap());
        let _ = m.is_break_classified(cl.class, cl.lookback);
    }
    m
}

fn parts_after(text: &[u32], cp: char, cells: ClusterCells) -> bool {
    let before = machine_after(text);
    let c = Codepoint::from_char(cp);
    let cl = Classified::of(c);
    let mut m = before;
    !m.is_break_classified(cl.class, cl.lookback)
        && before.parts_at(cells, cl.class, cl.lookback, c)
}

/// **Which characters take a column of their own after a letter, over the
/// whole code space**: after `a`, a character parts under legacy clusters
/// iff it is a spacing mark -- a SpacingMark (the `Mc` vowel signs and the
/// visarga, and the Thai and Lao SARA AM, U+0E33 and U+0EB3, which are
/// `Lo`) or an `Mc` that UAX #29 classes Extend (U+09BE BENGALI VOWEL SIGN
/// AA, U+0BBE TAMIL VOWEL SIGN AA, the Kannada vowel signs), which glibc
/// and vim count a column; after `क्`, a consonant that GB9c would join
/// parts too. Nothing else does: an emoji modifier, a ZWJ sequence, a
/// regional-indicator pair, a Hangul jamo and the halfwidth katakana voiced
/// marks stay with the character before them, joined by rules both variants
/// keep. With spacing marks joined, only the consonants part.
///
/// Every character that parts is narrow under every width policy, so a
/// part is one column, but four East Asian Wide `Mc`, the Hangul tone marks
/// U+302E-302F and U+16FF0-16FF1, which take the two columns glibc gives
/// them.
#[test]
fn only_spacing_marks_and_conjunct_consonants_part_after_a_letter() {
    let after_a = machine_after(&[0x61]);
    let after_ka_virama = machine_after(&[0x0915, 0x094D]);
    let (mut marks, mut consonants) = (0usize, 0usize);
    let mut wide = Vec::new();
    for v in 0x80u32..0x11_0000 {
        let Some(cp) = Codepoint::new(v) else {
            continue;
        };
        let cl = Classified::of(cp);
        let spacing = cl.class == GraphemeClass::SpacingMark
            || (cl.class == GraphemeClass::Extend && baud_domain::is_spacing_extend(cp));
        let consonant = cl.lookback.consonant();
        for (before, conjunct) in [(after_a, false), (after_ka_virama, true)] {
            let mut m = before;
            let joins = !m.is_break_classified(cl.class, cl.lookback);
            let legacy = joins && before.parts_at(ClusterCells::Legacy, cl.class, cl.lookback, cp);
            let joined = joins
                && before.parts_at(ClusterCells::LegacyMarksJoined, cl.class, cl.lookback, cp);
            assert_eq!(
                legacy,
                spacing || (conjunct && consonant),
                "U+{v:04X} legacy"
            );
            assert_eq!(joined, conjunct && consonant, "U+{v:04X} marks joined");
        }
        if spacing || consonant {
            for policy in [
                WidthPolicy::DEFAULT,
                WidthPolicy::of(AmbiguousWidth::Wide, UnicodeWidths::Current),
            ] {
                match cl.width(policy) == Width::NARROW {
                    true => {}
                    false if cl.width(policy) == Width::WIDE => wide.push(v),
                    false => panic!("U+{v:04X} is neither narrow nor wide"),
                }
            }
        }
        marks += usize::from(spacing);
        consonants += usize::from(consonant);
    }
    // `Grapheme_Cluster_Break=SpacingMark`, and `Mc` and `Extend`, and
    // `InCB=Consonant` in the pinned GraphemeBreakProperty.txt,
    // DerivedGeneralCategory.txt and DerivedCoreProperties.txt.
    assert_eq!(marks, 381 + 61, "spacing marks");
    assert_eq!(consonants, 911, "consonants");
    wide.dedup();
    assert_eq!(wide, [0x302E, 0x302F, 0x16FF0, 0x16FF1], "the wide ones");
    for (cp, why) in [
        ('\u{1F3FD}', "an emoji modifier"),
        ('\u{FF9E}', "a halfwidth katakana voiced mark"),
    ] {
        assert!(!parts_after(&[0x61], cp, ClusterCells::Legacy), "{why}");
    }
    assert!(
        !parts_after(&[0x1100], '\u{1161}', ClusterCells::Legacy),
        "a Hangul V"
    );
    assert!(
        !parts_after(&[0x1F1E6], '\u{1F1E8}', ClusterCells::Legacy),
        "a flag"
    );
    assert!(
        !parts_after(&[0x1F468, 0x200D], '\u{1F469}', ClusterCells::Legacy),
        "a ZWJ sequence"
    );
    for cp in ['\u{0E33}', '\u{0EB3}', '\u{0903}'] {
        assert!(parts_after(&[0x0E01], cp, ClusterCells::Legacy), "{cp:?}");
    }
    for (base, cp) in [
        (0x0995, '\u{09BE}'),
        (0x0B95, '\u{0BBE}'),
        (0x0C95, '\u{0CC0}'),
        (0x0D15, '\u{0D3E}'),
    ] {
        assert!(parts_after(&[base], cp, ClusterCells::Legacy), "{cp:?}");
        assert!(
            !parts_after(&[base], cp, ClusterCells::LegacyMarksJoined),
            "{cp:?}"
        );
        assert!(!parts_after(&[base], cp, ClusterCells::Extended), "{cp:?}");
    }
}

/// **After a Prepend everything that joins parts, but GB9's own**: U+0600
/// ARABIC NUMBER SIGN then U+0661 parts, as `a` would, while a mark after
/// the Prepend stays on it under every variant.
#[test]
fn a_prepend_parts_from_what_follows_it() {
    for cells in [ClusterCells::Legacy, ClusterCells::LegacyMarksJoined] {
        assert!(parts_after(&[0x0600], '\u{0661}', cells));
        assert!(parts_after(&[0x0600], '\u{61}', cells));
        assert!(
            !parts_after(&[0x0600], '\u{064B}', cells),
            "a mark joins by GB9"
        );
    }
    assert!(!parts_after(&[0x0600], '\u{0661}', ClusterCells::Extended));
}

/// The setting and the mode choose the cells.
#[test]
fn the_mode_and_the_setting_choose_the_cells() {
    use baud_domain::{GraphemeMode, SpacingMarks};
    assert_eq!(
        ClusterCells::of(GraphemeMode::Legacy, SpacingMarks::Column),
        ClusterCells::Legacy
    );
    assert_eq!(
        ClusterCells::of(GraphemeMode::Legacy, SpacingMarks::Joined),
        ClusterCells::LegacyMarksJoined
    );
    for m in [SpacingMarks::Column, SpacingMarks::Joined] {
        assert_eq!(
            ClusterCells::of(GraphemeMode::Extended, m),
            ClusterCells::Extended
        );
    }
    assert_eq!(ClusterCells::default(), ClusterCells::Legacy);
}

/// Which inputs -- class and lookback, by their places in [`inputs`] --
/// some codepoint in a block `in_block` answers no for takes, and the
/// first codepoint taking each.
fn inputs_outside(in_block: impl Fn(u32) -> bool) -> Vec<((GraphemeClass, Lookback), u32)> {
    let ins = inputs();
    let mut first: Vec<Option<u32>> = vec![None; ins.len()];
    for v in (0..=0x10_FFFFu32).filter(|&v| !in_block(v)) {
        let Some(c) = Codepoint::new(v) else { continue };
        let (class, lb) = GraphemeClass::of_with_lookback(c);
        let at = ins
            .iter()
            .position(|&(k, l)| k == class && l == lb)
            .expect("every class and lookback is an input");
        first[at].get_or_insert(v);
    }
    ins.into_iter()
        .zip(first)
        .filter_map(|(i, f)| f.map(|v| (i, v)))
        .collect()
}

/// The first (state, input) the machine reaches from the start of text on
/// `ins` alone at which a legacy cluster parts: the codepoint that parts,
/// or none.
fn first_part(ins: &[((GraphemeClass, Lookback), u32)]) -> Option<u32> {
    let mut seen: HashSet<ClusterBreak> = HashSet::new();
    let mut queue: VecDeque<ClusterBreak> = [ClusterBreak::after_control()].into();
    while let Some(m) = queue.pop_front() {
        if !seen.insert(m) {
            continue;
        }
        for &((class, lb), v) in ins {
            let mut next = m;
            let joins = !next.is_break_classified(class, lb);
            if joins && m.parts(ClusterCells::Legacy, class, lb) {
                return Some(v);
            }
            queue.push_back(next);
        }
    }
    None
}

/// **Text none of whose characters is in a block `SYLLABLE_BLOCKS` sets is
/// never parted, and holds no Devanagari letter**: from the start of text,
/// every state the machine reaches on the inputs such characters take, at
/// every one of those inputs, joins without parting, and none of them is a
/// spacing mark classed Extend or in `DEVANAGARI_RANGES` -- so a renderer
/// re-forming syllables and words lets a row of them go unread. The
/// control: the same walk with the blocks of U+0600, a Prepend, left out
/// of the table finds a part, U+0600 and then what follows it.
#[test]
fn text_outside_the_syllable_blocks_never_parts() {
    use baud_domain::{DEVANAGARI_RANGES, in_syllable_block, is_spacing_extend};
    let outside = inputs_outside(in_syllable_block);
    assert_eq!(first_part(&outside), None, "a part outside the table");
    for v in (0..=0x10_FFFFu32).filter(|&v| !in_syllable_block(v)) {
        let Some(c) = Codepoint::new(v) else { continue };
        assert!(!is_spacing_extend(c), "{v:04X}: a spacing Extend");
        assert!(
            !DEVANAGARI_RANGES.iter().any(|&(a, z)| a <= v && v <= z),
            "{v:04X}: a Devanagari letter"
        );
    }
    // Every word past U+10FFFF -- a blank, a wide tail, a cluster's -- is
    // answered no, by the zero word past the table.
    for w in [0x11_0000, 0x11_0001, 0x12_0001, 0x13_0000, u32::MAX] {
        assert!(!in_syllable_block(w), "{w:X}");
    }
    let control = inputs_outside(|v| in_syllable_block(v) && v >> 8 != 0x06);
    assert!(
        first_part(&control).is_some(),
        "the control: the Prepend's block left out finds a part"
    );
    // And ASCII, Han, Hebrew, box drawing and braille are outside it.
    for v in [0x41, 0x4E2D, 0x05D0, 0x2500, 0x2800] {
        assert!(!in_syllable_block(v), "{v:04X}");
    }
}

/// **The row table is the layout table and the syllable table side by
/// side**: at every block, `RowBlocks` answers what `in_layout_block` and
/// `in_syllable_block` answer, and for every word past U+10FFFF neither --
/// so the layout's one pass a row asks both. The control: some block
/// answers each alone.
#[test]
fn the_row_blocks_are_the_layout_and_syllable_blocks() {
    use baud_domain::{RowBlocks, in_layout_block, in_syllable_block};
    let (mut layout_only, mut syllables_only) = (0, 0);
    for b in 0..0x1100u32 {
        let w = b << 8;
        let row = RowBlocks::of(w);
        assert_eq!(row.layout(), in_layout_block(w), "{w:X}: layout");
        assert_eq!(row.syllables(), in_syllable_block(w), "{w:X}: syllables");
        layout_only += usize::from(row.layout() && !row.syllables());
        syllables_only += usize::from(row.syllables() && !row.layout());
    }
    assert!(layout_only > 0 && syllables_only > 0, "the control");
    for w in [0x11_0000, 0x11_0001, 0x12_0001, 0x13_0000, u32::MAX] {
        assert_eq!(RowBlocks::of(w), RowBlocks::NONE, "{w:X}");
    }
}
