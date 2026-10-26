//! UAX #9, the Unicode Bidirectional Algorithm, against Unicode's two
//! conformance files, every line of each (`[[source: unicode-biditest]]`,
//! `[[source: unicode-bidicharactertest]]`): the resolved level of every
//! character X9 keeps, and the order L2 displays them in, each text one
//! paragraph on one line. The characters X9 removes carry `x` and are left
//! out of the order, as both files say.

use baud_domain::{Bidi, BidiClass, BidiLevel, BidiParagraph, BidiTooLong};

fn source(name: &str) -> String {
    let path = format!(
        "{}/../docs/research/sources/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Removed by X9, and so `x` in both files.
fn removed(c: BidiClass) -> bool {
    use BidiClass::*;
    matches!(c, RLE | LRE | RLO | LRO | PDF | BN)
}

/// A level list as the files write it, `x` for none.
fn levels(field: &str) -> Vec<Option<u8>> {
    field
        .split_whitespace()
        .map(|t| (t != "x").then(|| t.parse().expect("a level")))
        .collect()
}

fn indices(field: &str) -> Vec<usize> {
    field
        .split_whitespace()
        .map(|t| t.parse().expect("an index"))
        .collect()
}

/// The levels and order `bidi` gives the text it last resolved, as one
/// line, in the files' form.
fn run<const N: usize>(bidi: &mut Bidi<N>, classes: &[BidiClass]) -> (Vec<Option<u8>>, Vec<usize>) {
    let mut order = vec![0u16; classes.len()];
    let line = bidi
        .reorder(0..classes.len(), &mut order)
        .expect("fits")
        .to_vec();
    let got = line
        .iter()
        .zip(classes)
        .map(|(l, &c)| (!removed(c)).then(|| l.number()))
        .collect();
    let order = order
        .iter()
        .map(|&i| usize::from(i))
        .filter(|&i| !removed(classes[i]))
        .collect();
    (got, order)
}

fn report(wrong: &[String], of: usize) {
    assert!(
        wrong.is_empty(),
        "{} of {of} runs differ:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// **Every case of `BidiTest.txt` 17.0.0 resolves and reorders as it
/// says**, at each paragraph level its bitset names: auto (P2-P3), LTR and
/// RTL. The positive control: the file has 490,846 data lines, which run
/// 770,241 times, and both counts are asserted, so a parser that stopped
/// reading would fail here rather than pass on less.
#[test]
fn every_bidi_test_case_resolves_and_reorders_as_it_says() {
    let text = source("unicode-biditest.txt");
    let mut bidi = Box::new(Bidi::<256>::new());
    let (mut want_levels, mut want_order) = (Vec::new(), Vec::new());
    let (mut lines, mut runs, mut wrong) = (0, 0, Vec::new());
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("@Levels:") {
            want_levels = levels(rest);
            continue;
        }
        if let Some(rest) = line.strip_prefix("@Reorder:") {
            want_order = indices(rest);
            continue;
        }
        if line.is_empty() || line.starts_with('@') {
            continue;
        }
        let (input, bits) = line.split_once(';').expect("a bitset");
        let classes: Vec<BidiClass> = input
            .split_whitespace()
            .map(|c| BidiClass::from_name(c).expect("a class"))
            .collect();
        let bits = u8::from_str_radix(bits.trim(), 16).expect("hex");
        assert_eq!(classes.len(), want_levels.len(), "line {}", n + 1);
        lines += 1;
        for (bit, dir) in [
            (1, BidiParagraph::FirstStrong),
            (2, BidiParagraph::Ltr),
            (4, BidiParagraph::Rtl),
        ] {
            if bits & bit == 0 {
                continue;
            }
            runs += 1;
            bidi.resolve_classes(&classes, dir).expect("fits");
            let (got, order) = run(&mut bidi, &classes);
            if got != want_levels || order != want_order {
                wrong.push(format!(
                    "line {} {dir:?}: {input}\n  levels {got:?} want {want_levels:?}\n  order {order:?} want {want_order:?}",
                    n + 1
                ));
            }
        }
    }
    assert_eq!((lines, runs), (490_846, 770_241));
    report(&wrong, runs);
}

/// **Every case of `BidiCharacterTest.txt` 17.0.0 resolves and reorders as
/// it says**: the paragraph level (field 2), each character's level and
/// the order, over code points, so the paired brackets of N0 and BD16 are
/// read from the tables. The positive control: 91,707 cases, asserted.
#[test]
fn every_bidi_character_test_case_resolves_and_reorders_as_it_says() {
    let text = source("unicode-bidicharactertest.txt");
    let mut bidi = Box::new(Bidi::<256>::new());
    let (mut lines, mut wrong) = (0, Vec::new());
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').collect();
        assert_eq!(f.len(), 5, "line {}", n + 1);
        let chars: Vec<char> = f[0]
            .split_whitespace()
            .map(|h| char::from_u32(u32::from_str_radix(h, 16).expect("hex")).expect("a char"))
            .collect();
        let dir = match f[1] {
            "0" => BidiParagraph::Ltr,
            "1" => BidiParagraph::Rtl,
            _ => BidiParagraph::FirstStrong,
        };
        let want_para: u8 = f[2].parse().expect("a level");
        let (want_levels, want_order) = (levels(f[3]), indices(f[4]));
        lines += 1;
        let para = bidi.resolve(&chars, dir).expect("fits");
        let classes: Vec<BidiClass> = chars.iter().map(|&c| BidiClass::of(c)).collect();
        let (got, order) = run(&mut bidi, &classes);
        if para.number() != want_para || got != want_levels || order != want_order {
            wrong.push(format!(
                "line {}: {}\n  paragraph {} want {want_para}\n  levels {got:?} want {want_levels:?}\n  order {order:?} want {want_order:?}",
                n + 1,
                f[0],
                para.number()
            ));
        }
    }
    assert_eq!(lines, 91_707);
    report(&wrong, lines);
}

/// **The annex's own examples**, a positive control that needs neither
/// file: L2's example 1, "car means CAR." at level 0, and BD7's, "car is
/// THE CAR in arabic", with the uppercase as Hebrew letters.
#[test]
fn the_annex_examples_resolve_as_it_shows() {
    let mut bidi = Box::new(Bidi::<64>::new());
    let text: Vec<char> = "car means \u{05D0}\u{05D1}\u{05D2}.".chars().collect();
    let level = bidi
        .resolve(&text, BidiParagraph::FirstStrong)
        .expect("fits");
    assert_eq!(level, BidiLevel::LTR);
    let mut order = vec![0u16; text.len()];
    let levels: Vec<u8> = bidi
        .reorder(0..text.len(), &mut order)
        .expect("fits")
        .iter()
        .map(|l| l.number())
        .collect();
    assert_eq!(levels, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0]);
    let shown: String = order.iter().map(|&i| text[usize::from(i)]).collect();
    assert_eq!(shown, "car means \u{05D2}\u{05D1}\u{05D0}.");

    let text: Vec<char> = "car is \u{05D0}\u{05D1}\u{05D2} \u{05D3}\u{05D4}\u{05D5} in arabic"
        .chars()
        .collect();
    bidi.resolve(&text, BidiParagraph::FirstStrong)
        .expect("fits");
    let levels: String = bidi
        .levels()
        .iter()
        .map(|l| char::from(b'0' + l.number()))
        .collect();
    assert_eq!(levels, "000000011111110000000000");
}

/// **L1 is per line**: a paragraph wrapped onto two lines resets the
/// whitespace at the end of each line to the paragraph level, where
/// resolved as one line it is between two right-to-left characters and
/// takes their level.
#[test]
fn a_line_ends_its_trailing_whitespace_at_the_paragraph_level() {
    use BidiClass::*;
    let mut bidi = Box::new(Bidi::<8>::new());
    let classes = [R, WS, R];
    bidi.resolve_classes(&classes, BidiParagraph::Ltr)
        .expect("fits");
    let mut order = [0u16; 3];
    let whole: Vec<u8> = bidi
        .reorder(0..3, &mut order)
        .expect("fits")
        .iter()
        .map(|l| l.number())
        .collect();
    assert_eq!((whole, order), (vec![1, 1, 1], [2, 1, 0]));
    let mut order = [0u16; 2];
    let first: Vec<u8> = bidi
        .reorder(0..2, &mut order)
        .expect("fits")
        .iter()
        .map(|l| l.number())
        .collect();
    assert_eq!((first, order), (vec![1, 0], [0, 1]));
    let mut order = [0u16; 1];
    bidi.reorder(2..3, &mut order).expect("fits");
    assert_eq!(order, [2]);
}

/// **P1 splits paragraphs at a paragraph separator**, and each finds its
/// own level by P2-P3: Hebrew then U+2029 then Latin is a level-1
/// paragraph and a level-0 one.
#[test]
fn each_paragraph_takes_its_own_level() {
    let mut bidi = Box::new(Bidi::<8>::new());
    let text = ['\u{05D0}', '\u{2029}', 'a'];
    let first = bidi
        .resolve(&text, BidiParagraph::FirstStrong)
        .expect("fits");
    assert_eq!(first, BidiLevel::RTL);
    assert_eq!(bidi.paragraph_level(0), Some(BidiLevel::RTL));
    assert_eq!(bidi.paragraph_level(1), Some(BidiLevel::RTL));
    assert_eq!(bidi.paragraph_level(2), Some(BidiLevel::LTR));
    assert_eq!(bidi.paragraph_level(3), None);
}

/// **A text longer than the capacity is refused whole**, and one that
/// fits it exactly is resolved: a terminal row at the grid's widest,
/// 2048 cells, fits a `Bidi<2048>`. An order too short for its line is
/// refused too, where cutting the line would move L1's line end.
#[test]
fn a_text_past_the_capacity_is_refused() {
    let mut bidi = Box::new(Bidi::<2048>::new());
    let row = vec!['\u{05D0}'; 2048];
    assert_eq!(
        bidi.resolve(&row, BidiParagraph::FirstStrong),
        Ok(BidiLevel::RTL)
    );
    let mut order = vec![0u16; 2048];
    bidi.reorder(0..2048, &mut order).expect("fits");
    assert_eq!((order[0], order[2047]), (2047, 0));
    let mut short = vec![0u16; 2047];
    assert_eq!(
        bidi.reorder(0..2048, &mut short),
        Err(BidiTooLong {
            len: 2048,
            capacity: 2047
        })
    );
    let long = vec!['a'; 2049];
    assert_eq!(
        bidi.resolve(&long, BidiParagraph::FirstStrong),
        Err(BidiTooLong {
            len: 2049,
            capacity: 2048
        })
    );
}

/// **P2-P3 with a right-to-left default** (`BidiParagraph::FirstStrongRtl`,
/// terminal-wg's autodetection over a right-to-left path): a text with no
/// strong character, or whose only strong one sits inside an isolate, is
/// level 1; one that starts in a Latin letter is level 0, and a Hebrew one
/// level 1, as with the left-to-right default.
#[test]
fn a_right_to_left_default_takes_a_text_with_no_strong_letter() {
    let mut bidi = Box::new(Bidi::<16>::new());
    let mut level = |t: &str, dir| {
        bidi.resolve(&t.chars().collect::<Vec<_>>(), dir)
            .expect("fits")
    };
    assert_eq!(level("123 ", BidiParagraph::FirstStrongRtl), BidiLevel::RTL);
    assert_eq!(
        level("\u{2066}ab", BidiParagraph::FirstStrongRtl),
        BidiLevel::RTL
    );
    assert_eq!(
        level("\u{2066}ab\u{2069}", BidiParagraph::FirstStrongRtl),
        BidiLevel::RTL
    );
    assert_eq!(level("1 ab", BidiParagraph::FirstStrongRtl), BidiLevel::LTR);
    assert_eq!(
        level("1 \u{5D0}", BidiParagraph::FirstStrongRtl),
        BidiLevel::RTL
    );
    assert_eq!(
        level("123 ", BidiParagraph::FirstStrong),
        BidiLevel::LTR,
        "the control"
    );
}

/// **The one-pass direction is the algorithm's**: `paragraph_direction`
/// against the level `Bidi::resolve` gives, over every text of the
/// character test file, at both defaults.
#[test]
fn the_one_pass_direction_is_the_algorithms_on_every_character_test_text() {
    let text = source("unicode-bidicharactertest.txt");
    let mut bidi = Box::new(Bidi::<256>::new());
    let mut lines = 0;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let chars: Vec<char> = line
            .split(';')
            .next()
            .expect("a field")
            .split_whitespace()
            .map(|h| char::from_u32(u32::from_str_radix(h, 16).expect("hex")).expect("a char"))
            .collect();
        for dir in [BidiParagraph::FirstStrong, BidiParagraph::FirstStrongRtl] {
            let level = bidi.resolve(&chars, dir).expect("fits");
            assert_eq!(
                baud_domain::paragraph_direction(chars.iter().copied(), dir),
                level.direction(),
                "{line}"
            );
        }
        lines += 1;
    }
    assert_eq!(lines, 91_707);
}
