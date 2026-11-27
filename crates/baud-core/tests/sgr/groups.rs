//! A colon group is one attribute, and the walk goes on past one that
//! applies nothing.
//!
//! `[[source: xterm-sgr]]`'s `CASE_SGR` (charproc.c) walks the parameters
//! one attribute at a time: "Only SGR 38/48 accept subparameters", and any
//! other head carrying them is skipped with its whole group (`item += skip;
//! /* ignore this */`). A head it does not know falls to `default: break;`,
//! inside the loop, so the attributes after it still apply. baud takes
//! sub-parameters on `4` (`[[source: kitty-underlines]]`) and `58` as well;
//! every other head with a group applies nothing and takes its group with
//! it. Only a colour introducer whose space is missing or unknown ends the
//! walk, as it always has.

use crate::support;
use baud_core::*;
use baud_domain::ParamIdx;

/// The rendition an `x` printed after `sgr` takes.
fn style(sgr: &str) -> StyleWord {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, format!("\x1b[{sgr}mx").as_bytes());
    t.styles().one().word_of(support::screen(&t)[0])
}

fn colon(v: &[u16]) -> String {
    v.iter().map(u16::to_string).collect::<Vec<_>>().join(":")
}

/// **However long the group, its last sub-parameter is not an attribute.**
/// For each head that takes sub-parameters, the shortest form of it, padded
/// and ended by a `1`, out to every length the parameter array holds: the
/// rendition is the shortest form's, with no bold. The group's extent
/// stopped at a literal sixteen, so from seventeen on the tail was read as
/// attributes of their own. Padded with `0` and with `4`, which is itself a
/// head that takes sub-parameters, so a walk that lands inside the tail
/// cannot pass by skipping what it lands on for taking none.
#[test]
fn a_colon_group_is_one_attribute_however_long() {
    let shortest: [&[u16]; 4] = [&[4, 3], &[38, 5, 9], &[48, 5, 9], &[58, 5, 9]];
    for form in shortest {
        let want = style(&colon(form));
        for (pad, g) in [0, 4]
            .into_iter()
            .flat_map(|p| (form.len() + 1..=ParamIdx::CAPACITY).map(move |g| (p, g)))
        {
            let mut group = form.to_vec();
            group.resize(g - 1, pad);
            group.push(1);
            assert_eq!(
                style(&colon(&group)),
                want,
                "{} sub-parameters after {}, padded with {pad}",
                g - 1,
                form[0]
            );
        }
    }
}

/// **A head that takes no sub-parameters applies nothing, with its group.**
/// `1:5` is neither bold nor blinking, and `22:1` after a bold leaves the
/// bold alone. The control: `1;5`, two attributes, is both.
#[test]
fn a_group_on_a_head_that_takes_none_applies_nothing() {
    let plain = style("");
    assert_eq!(style("1:5"), plain, "1:5 sets nothing");
    assert_eq!(style("1;22:1"), style("1"), "22:1 neither clears nor sets");
    assert_ne!(style("1;5"), plain, "the control: 1;5 sets two attributes");
    assert_eq!(style("1;5"), style("5;1"));
}

/// **An unknown head applies nothing, and what follows it still applies.**
/// `66` names no attribute, bare or with a group, and the italic after it
/// is set as it is with nothing between.
#[test]
fn an_unknown_head_does_not_end_the_walk() {
    let want = style("1;3");
    assert_ne!(want, style("1"), "the control: 3 is italic");
    assert_eq!(style("1;66;3"), want, "66 skipped, 3 applied");
    assert_eq!(style("1;66:5;3"), want, "66:5 skipped whole, 3 applied");
}

/// **A colour with no space it knows still ends the walk.** `38;7` names
/// neither `2` nor `5`, so where its channels end cannot be known, and
/// nothing after it is read.
#[test]
fn a_malformed_colour_ends_the_walk() {
    assert_eq!(style("1;38;7;3"), style("1"));
    assert_eq!(style("1;38:7:3;3"), style("1"));
}
