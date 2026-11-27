//! `vs16::BASES` is `[[source: emoji-variation-sequences]]`'s emoji-style
//! bases, keycaps excluded: a table written beside the file it comes from
//! drifts from it immediately unless something holds the two together.

use baud_domain::Codepoint;
use baud_domain::vs16;

fn pinned() -> Vec<u32> {
    let path = format!(
        "{}/../../docs/research/sources/unicode-emoji-variation-sequences.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out: Vec<u32> = text
        .lines()
        .map(|l| l.split('#').next().unwrap().trim())
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let mut f = l.split(';').map(str::trim);
            let seq = f.next()?;
            let kind = f.next()?;
            let mut cps = seq.split_whitespace();
            let base = u32::from_str_radix(cps.next()?, 16).ok()?;
            (cps.next() == Some("FE0F") && kind == "emoji style").then_some(base)
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn keycap(v: u32) -> bool {
    v == 0x23 || v == 0x2a || (0x30..=0x39).contains(&v)
}

#[test]
fn the_vs16_bases_are_the_pinned_files_emoji_style_sequences() {
    let all = pinned();
    assert!(
        all.len() > 350,
        "positive control: the file parsed ({})",
        all.len()
    );
    assert_eq!(
        all.iter().filter(|&&v| keycap(v)).count(),
        12,
        "the twelve keycap bases are in the file"
    );
    let want: Vec<u32> = all.into_iter().filter(|&v| !keycap(v)).collect();
    assert_eq!(vs16::BASES.to_vec(), want);
    assert!(
        vs16::BASES.windows(2).all(|w| w[0] < w[1]),
        "sorted for the search"
    );
}

#[test]
fn widens_answers_the_table() {
    for (c, want) in [
        ('\u{2764}', true),
        ('\u{2620}', true),
        ('\u{a9}', true),
        ('A', false),
        ('1', false),
        ('#', false),
        ('\u{6c34}', false),
    ] {
        assert_eq!(vs16::widens(Codepoint::from_char(c)), want, "{c:?}");
    }
}

/// **`Vs16Width::One` keeps one cell only while `?2027` is reset**: set
/// ([`baud_domain::ClusterCells::Extended`]), the mode's VS16 rule widens the
/// base under either setting (`[[source: mode2027]]`).
#[test]
fn one_cell_holds_only_outside_grapheme_cluster_mode() {
    use baud_domain::{ClusterCells, Vs16Width};
    for cells in [
        ClusterCells::Legacy,
        ClusterCells::LegacyMarksJoined,
        ClusterCells::Extended,
    ] {
        assert!(Vs16Width::Two.widens_in(cells), "{cells:?}");
        assert_eq!(
            Vs16Width::One.widens_in(cells),
            cells == ClusterCells::Extended,
            "{cells:?}"
        );
    }
}

#[test]
fn the_emoji_presentation_ranges_are_the_pinned_files() {
    let path = format!(
        "{}/../../docs/research/sources/unicode-emoji-data.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let mut set = std::collections::BTreeSet::new();
    for l in text.lines() {
        let l = l.split('#').next().unwrap().trim();
        let Some((cps, prop)) = l.split_once(';') else {
            continue;
        };
        if prop.trim() != "Emoji_Presentation" {
            continue;
        }
        let (lo, hi) = match cps.trim().split_once("..") {
            Some((a, b)) => (a, b),
            None => (cps.trim(), cps.trim()),
        };
        let (lo, hi) = (
            u32::from_str_radix(lo, 16).unwrap(),
            u32::from_str_radix(hi, 16).unwrap(),
        );
        set.extend(lo..=hi);
    }
    assert!(
        set.len() > 1000,
        "positive control: the file parsed ({})",
        set.len()
    );
    for v in 0..=0x10ffffu32 {
        if let Some(cp) = Codepoint::new(v) {
            assert_eq!(vs16::presents_as_emoji(cp), set.contains(&v), "U+{v:04X}");
        }
    }
}
