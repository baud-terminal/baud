//! A precomposed letter and its canonical decomposition are one text
//! (`[[source: unicode18-ch3]]` D70, C6), so the shaper's normalisation
//! hands a face the same thing for both; and a letter the face cannot draw
//! in parts is the one `.notdef` that stands for it
//! (`[[source: unicode18-ch5]]` 5.3), as HarfBuzz's `decompose` keeps it
//! (`[[source: harfbuzz-hb-ot-shape-normalize]]`).
//!
//! JetBrains Mono has neither Ȃ (U+0202) nor the inverted breve (U+0311),
//! neither the Hangul syllables nor their jamo, and has Å and the acute.

use baud_domain::{Codepoint, GlyphId};
use baud_font::{Cluster, Face, Features, Glyph, Ignorable, Normalised, Script, Shaper};

fn cp(c: u32) -> Codepoint {
    Codepoint::new(c).unwrap()
}

fn cps(s: &[u32]) -> Vec<Codepoint> {
    s.iter().map(|&c| cp(c)).collect()
}

fn values(s: &[Codepoint]) -> Vec<u32> {
    s.iter().map(|c| c.value()).collect()
}

fn jbm() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/JetBrainsMono-Regular.ttf"
    ))
    .unwrap()
}

fn premises(face: &Face<'_>) {
    for c in [0x0202, 0x0311, 0xC548, 0x110B, 0x1161, 0x11AB, 0x01FA] {
        assert!(face.glyph(cp(c)).is_none(), "the premise: no U+{c:04X}");
    }
    for c in [0x41, 0xC5, 0x301] {
        assert!(face.glyph(cp(c)).is_some(), "the premise: U+{c:04X}");
    }
}

/// **A letter the face cannot draw in parts stays whole**, and shapes to
/// one `.notdef`: before, Ȃ came apart into an `A` and a `.notdef` for the
/// breve, which JetBrains Mono advances a full cell, so the box drew over
/// the next cell.
#[test]
fn a_letter_the_face_cannot_draw_in_parts_stays_whole() {
    let data = jbm();
    let face = Face::parse(&data).unwrap();
    premises(&face);
    for c in [0x0202, 0xC548] {
        assert_eq!(
            values(Normalised::of(&face, &cps(&[c])).as_slice()),
            vec![c],
            "U+{c:04X}"
        );
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 16];
        let mut shaper = Shaper::new(&face, &mut storage);
        shaper.push_cluster(&cps(&[c]), Cluster(0)).unwrap();
        let (buf, _) = shaper.finish(Features::GRID, Script::of(cp(c)));
        let ids: Vec<GlyphId> = buf.as_slice().iter().map(|g| g.id).collect();
        assert_eq!(ids, vec![GlyphId::NOTDEF], "U+{c:04X}: one box");
    }
    // The control: a letter it draws in parts still comes apart.
    assert_eq!(
        values(Normalised::of(&face, &cps(&[0x01FA])).as_slice()),
        vec![0xC5, 0x301],
        "Ǻ is Å and the acute"
    );
}

/// **Canonical equivalents lose the same marks.** What the renderer hands
/// the shaper for a cluster no face draws whole starts from its canonical
/// form, so A + U+0311 is Ȃ, the jamo of 안 are 안, and α + grave +
/// ypogegrammeni is ᾲ: one `.notdef` each, nothing left out -- where the
/// typed decomposition used to lose its marks and the precomposed letter
/// to draw its parts' boxes.
#[test]
fn canonical_equivalents_lose_the_same_marks() {
    let data = jbm();
    let face = Face::parse(&data).unwrap();
    premises(&face);
    for (typed, letter) in [
        (&[0x41, 0x311][..], 0x0202),
        (&[0x110B, 0x1161, 0x11AB], 0xC548),
        (&[0x3B1, 0x300, 0x345], 0x1FB2),
        (&[0x3B1, 0x345, 0x300], 0x1FB2),
    ] {
        for form in [typed, &[letter][..]] {
            let kept = Normalised::without_unmapped(&face, &cps(form));
            assert_eq!(values(kept.as_slice()), vec![letter], "{form:04X?}");
            assert!(kept.dropped().is_zero(), "{form:04X?}: nothing left out");
        }
    }
    // The control: a mark with no composite is still left out.
    let kept = Normalised::without_unmapped(&face, &cps(&[0x71, 0x316]));
    assert_eq!(values(kept.as_slice()), vec![0x71]);
    assert!(!kept.dropped().is_zero());
}

/// `Normalised::comes_apart`, which the face table asks of every
/// precomposed character, answers what normalising the character alone and
/// asking `covered` does, for every character with a canonical
/// decomposition that JetBrains Mono or Fira Code lacks.
#[test]
fn comes_apart_is_what_covered_answers() {
    let fira = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/FiraCode-Regular.ttf"
    ))
    .unwrap();
    let data = jbm();
    let mut checked = 0;
    let mut apart = 0;
    for face in [Face::parse(&data).unwrap(), Face::parse(&fira).unwrap()] {
        baud_font::each_decomposable(|lo, hi| {
            for v in lo..=hi {
                let c = cp(v);
                if face.glyph(c).is_some() {
                    continue;
                }
                let covered = Normalised::of(&face, &[c]).covered();
                assert_eq!(Normalised::comes_apart(&face, c), covered, "U+{v:04X}");
                checked += 1;
                apart += usize::from(covered);
            }
        });
    }
    assert!(
        checked > 10_000 && apart > 100,
        "the premise: {checked} characters checked, {apart} come apart"
    );
}
