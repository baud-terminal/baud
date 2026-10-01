//! `vs16::BASES` is `[[source: emoji-variation-sequences]]`'s emoji-style
//! bases, keycaps excluded: a table written beside the file it comes from
//! drifts from it immediately unless something holds the two together.

use baud_domain::Codepoint;
use baud_domain::vs16;

fn pinned() -> Vec<u32> {
    let path = format!(
        "{}/../docs/research/sources/unicode-emoji-variation-sequences.txt",
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
