//! **The screen gates of the cores `equal::check` does not compare.** A
//! core's rate on a recording is printed only where it and baud-core drew
//! the same screen; where they did not, its row says `UNEQUAL` and prints
//! no rate, since that rate is a rate for different work.
//!
//! * **alacritty** is compared as `baud-difftest` compares it: every cell's
//!   character and whether its background is the default. The second half
//!   of a wide character, which alacritty stores as a space flagged a
//!   spacer and baud-core as a wide tail, is the same cell; a tab alacritty
//!   writes into a blank is a blank.
//! * **ghostty** is `equal::check`'s, which governs whether a recording is
//!   timed at all.
//! * **vt100 and wezterm** are not compared, since no oracle reads their
//!   screens: their rows say `UNCHECKED`. termwiz is a parser and keeps no
//!   screen.
//!
//! Both arms are built as their timed reps are (`cores::baud_new`, the
//! alacritty arm of `cores::run_one`) and fed the whole replays of the
//! recording that fit in the run's bytes, in the reads the rates use, as
//! `equal::check` feeds them: never a replay cut inside a control string.

use crate::transport::through_memory;
use crate::{COLS, ROWS};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};

/// What a gate says about one recording.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Gate {
    /// The two final screens are equal.
    Same,
    /// The two final screens differ on this many rows.
    Unequal(Rows),
    /// No oracle reads this core's screen.
    Unchecked,
    /// The core is a parser and keeps no screen.
    NoScreen,
}

/// A count of screen rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Rows(pub(crate) usize);

impl Gate {
    /// Whether the core's rate on the recording is printed: everywhere but
    /// where its screen differs from baud-core's.
    pub(crate) fn rated(self) -> bool {
        !matches!(self, Gate::Unequal(_))
    }

    /// The words printed under a row for one recording.
    pub(crate) fn says(self, label: &str) -> String {
        match self {
            Gate::Same => format!("{label}: same screen as baud-core"),
            Gate::Unequal(Rows(n)) => {
                format!("{label}: UNEQUAL ({n} rows differ from baud-core's), no rate")
            }
            Gate::Unchecked => format!("{label}: UNCHECKED (no oracle reads this screen)"),
            Gate::NoScreen => format!("{label}: a parser, no screen to compare"),
        }
    }
}

/// The gate over two screens read the same way: `Same`, or how many rows
/// differ.
pub(crate) fn gate<T: PartialEq>(ours: &[Vec<T>], theirs: &[Vec<T>]) -> Gate {
    let differ = ours
        .iter()
        .zip(theirs.iter())
        .filter(|(a, b)| a != b)
        .count()
        + ours.len().abs_diff(theirs.len());
    match differ {
        0 => Gate::Same,
        n => Gate::Unequal(Rows(n)),
    }
}

/// The bytes the gates feed: the whole replays of `p` that fit in `total`,
/// one at least.
fn whole(p: &[u8], total: usize) -> usize {
    (total / p.len().max(1)).max(1) * p.len()
}

/// One alacritty-gate cell: the character drawn and whether the background
/// is set.
pub(crate) type ACell = (char, bool);

/// baud-core's screen after `bytes` of `p`, as `baud-difftest` reads it.
pub(crate) fn baud_for_alacritty(p: &[u8], bytes: usize) -> Vec<Vec<ACell>> {
    let ws = crate::cores::baud_ws();
    let mut owned = crate::cores::baud_owned(&ws);
    let mut t = crate::cores::baud_new(&mut owned, &ws);
    through_memory(p, bytes, |chunk| crate::cores::baud_feed(&mut t, chunk));
    (0..ROWS)
        .map(|y| {
            t.row(baud_core::Row::from_index(y))
                .expect("a row on the grid")[..COLS]
                .iter()
                .map(|c| {
                    let ch = t
                        .base_of(*c)
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or(if c.content.is_wide_tail() { '\0' } else { ' ' });
                    (ch, !t.styles().one().word_of(*c).bg().is_default())
                })
                .collect()
        })
        .collect()
}

/// alacritty's screen after `bytes` of `p`, as `baud-difftest` reads it,
/// configured as its timed arm is.
pub(crate) fn alacritty_screen(p: &[u8], bytes: usize) -> Vec<Vec<ACell>> {
    let cfg = Config {
        scrolling_history: 10_000,
        ..Default::default()
    };
    let mut t = ATerm::new(cfg, &crate::cores::Dim, crate::cores::Noop);
    let mut proc = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
    through_memory(p, bytes, |chunk| proc.advance(&mut t, chunk));
    let g = t.grid();
    (0..ROWS)
        .map(|y| {
            (0..COLS)
                .map(|x| {
                    let cell = &g[alacritty_terminal::index::Line(y as i32)]
                        [alacritty_terminal::index::Column(x)];
                    let ch = if cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                    {
                        '\0'
                    } else if cell.c == '\t' {
                        ' '
                    } else {
                        cell.c
                    };
                    (ch, cell.bg != Color::Named(NamedColor::Background))
                })
                .collect()
        })
        .collect()
}

/// The gate for core `ci` (the index `cores::run_one` matches on) over
/// recording `p` in a run of `total` bytes; `None` for a core no gate here
/// speaks for: the ceiling, baud-core itself, ghostty (`equal::check`) and
/// sfte (`sfte_admissible`).
pub(crate) fn for_core(ci: usize, p: &[u8], total: usize) -> Option<Gate> {
    match ci {
        2 => {
            let bytes = whole(p, total);
            Some(gate(
                &baud_for_alacritty(p, bytes),
                &alacritty_screen(p, bytes),
            ))
        }
        4 | 5 => Some(Gate::Unchecked),
        6 => Some(Gate::NoScreen),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text, a coloured background, a wide character and a tab: each of the
    /// cell kinds the alacritty reading normalises.
    const SAMPLE: &[u8] = "hello\r\nworld \x1b[41mred\x1b[m \u{4e2d}\tend\r\n".as_bytes();

    #[test]
    fn a_planted_wrong_cell_fails_the_alacritty_gate() {
        let ours = baud_for_alacritty(SAMPLE, SAMPLE.len());
        let theirs = alacritty_screen(SAMPLE, SAMPLE.len());
        assert_eq!(
            gate(&ours, &theirs),
            Gate::Same,
            "the control: equal work passes"
        );
        let mut planted = theirs.clone();
        planted[1][2].0 = 'X';
        assert_eq!(gate(&ours, &planted), Gate::Unequal(Rows(1)));
        let mut coloured = theirs;
        coloured[0][0].1 = !coloured[0][0].1;
        assert_eq!(
            gate(&ours, &coloured),
            Gate::Unequal(Rows(1)),
            "a background differs"
        );
    }

    #[test]
    fn the_alacritty_gate_compares_whole_replays_only() {
        // A run of 2.5 recordings' bytes feeds two whole replays.
        assert_eq!(whole(SAMPLE, SAMPLE.len() * 5 / 2), SAMPLE.len() * 2);
        // A run shorter than one recording still feeds one.
        assert_eq!(whole(SAMPLE, 1), SAMPLE.len());
    }

    #[test]
    fn an_unequal_row_prints_no_rate() {
        assert!(Gate::Same.rated());
        assert!(Gate::Unchecked.rated());
        assert!(Gate::NoScreen.rated());
        assert!(!Gate::Unequal(Rows(1)).rated());
    }
}
