//! `OSC 10` and `11` under `?5` act on the colour shown (note V, V17).
//!
//! xterm's `ReverseVideo` swaps the colours it *stores* -- palette entries 0
//! and 7 and 8 and 15, the text foreground and background, as whole records
//! with their defaults (`[[source: xterm-scroll]]` util.c 3269-3307,
//! `swapLocally` 3211-3241) -- and `ReverseOldColors` swaps the copies the
//! dynamic queries answer from (`[[source: xterm-misc]]` misc.c 6411-6445).
//! `ChangeColorsRequest`, `ReportColorRequest` and `ResetColorsRequest` then
//! index the swapped slots (misc.c 3568-3750: under `re_verse`,
//! `oppositeColor` returns `TEXT_FG` for `TEXT_FG`), so every set, query and
//! reset lands on the colour drawn, and a reply keeps the code the program
//! sent.
//!
//! baud stores the unreversed slots and exchanges every cell where it draws
//! (DEC STD 070 § 5.6.3.2; the owner's decision of 2026-10-13), so a default
//! cell shows the stored background as text and the stored foreground as
//! its page, and the same answer is a mapping at the slot the core reads or
//! writes: under `?5`, `OSC 10` names the stored background and `OSC 11` the
//! stored foreground. `OSC 4` is not mapped: under the exchange no palette
//! entry moves, so the entry a program names is the entry drawn.

mod support;

use baud_core::*;
use baud_domain::Video;

const FG: (u8, u8, u8) = (0xd8, 0xd8, 0xd8);
const BG: (u8, u8, u8) = (0x10, 0x10, 0x10);
const SET: (u8, u8, u8) = (0x12, 0x34, 0x56);

/// Entry `i` is `(i, 255 - i, i ^ 0x5a)`, as in `palette_query.rs`.
fn table() -> [(u8, u8, u8); 256] {
    core::array::from_fn(|i| {
        let i = u8::try_from(i).expect("256 entries");
        (i, 255 - i, i ^ 0x5a)
    })
}

fn hex4((r, g, b): (u8, u8, u8)) -> String {
    format!("rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}")
}

fn dynamic_reply(code: u8, colour: (u8, u8, u8)) -> Vec<u8> {
    format!("\x1b]{code};{}\x07", hex4(colour)).into_bytes()
}

/// A fresh terminal told the theme's colours and table, fed `bytes` at read
/// sizes 1, 3, 7 and whole.
fn each_read_size(bytes: &[u8], f: impl Fn(&Term<'_>)) {
    let ws = support::CORPUS;
    for size in [1, 3, 7, bytes.len().max(1)] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_default_colors(FG, BG);
        t.set_default_palette(&table());
        for chunk in bytes.chunks(size) {
            support::feed(&mut t, chunk);
        }
        f(&t);
    }
}

fn overrides(t: &Term<'_>) -> Vec<u8> {
    (0..=255u8)
        .filter(|&i| t.palette(Chan::from_u8(i)).is_some())
        .collect()
}

#[test]
fn osc_10_and_11_queries_answer_the_colours_shown() {
    // The control: unreversed, each answers its own role.
    each_read_size(b"\x1b]10;?\x07\x1b]11;?\x07", |t| {
        assert_eq!(
            t.reply(),
            [dynamic_reply(10, FG), dynamic_reply(11, BG)].concat()
        );
    });
    // Reversed, the background shown is the theme's foreground, and the
    // reply keeps the code the program asked with.
    each_read_size(b"\x1b[?5h\x1b]10;?\x07\x1b]11;?\x07", |t| {
        assert_eq!(
            t.reply(),
            [dynamic_reply(10, BG), dynamic_reply(11, FG)].concat()
        );
    });
}

#[test]
fn osc_11_under_reverse_sets_the_background_shown() {
    each_read_size(b"\x1b[?5h\x1b]11;rgb:12/34/56\x07\x1b]11;?\x07", |t| {
        // The background shown is the stored foreground.
        assert_eq!(t.dynamic_color(DynamicColor::Foreground), Some(SET));
        assert_eq!(t.dynamic_color(DynamicColor::Background), None);
        assert_eq!(t.reply(), dynamic_reply(11, SET));
    });
    each_read_size(b"\x1b[?5h\x1b]10;rgb:12/34/56\x07", |t| {
        assert_eq!(t.dynamic_color(DynamicColor::Background), Some(SET));
        assert_eq!(t.dynamic_color(DynamicColor::Foreground), None);
    });
}

/// "After `?5l`, whatever was set carries into the other role": what a
/// program set as the background while reversed is the text once the screen
/// is normal again, as in xterm, whose swap moves it back with its record.
#[test]
fn a_colour_set_under_reverse_carries_into_the_other_role() {
    each_read_size(
        b"\x1b[?5h\x1b]11;rgb:12/34/56\x07\x1b[?5l\x1b]10;?\x07",
        |t| {
            assert_eq!(t.reply(), dynamic_reply(10, SET));
        },
    );
}

#[test]
fn osc_110_and_111_under_reverse_reset_the_colour_shown() {
    // The stored foreground, set unreversed, is the background shown, so
    // `OSC 111` under `?5` clears it and `OSC 110` does not.
    each_read_size(b"\x1b]10;rgb:12/34/56\x07\x1b[?5h\x1b]110\x07", |t| {
        assert_eq!(t.dynamic_color(DynamicColor::Foreground), Some(SET));
    });
    each_read_size(b"\x1b]10;rgb:12/34/56\x07\x1b[?5h\x1b]111\x07", |t| {
        assert_eq!(t.dynamic_color(DynamicColor::Foreground), None);
    });
    each_read_size(b"\x1b]11;rgb:12/34/56\x07\x1b[?5h\x1b]110\x07", |t| {
        assert_eq!(t.dynamic_color(DynamicColor::Background), None);
    });
}

/// Both resets still clear every override, set reversed or not, and
/// `DECSTR` keeps `?5`: DEC STD 070 lists Screen Mode among the "User
/// Preference Features ... NOT affected by the soft reset"
/// (`[[source: dec-std-070]]`, PDF 235), and xterm's soft branch leaves it
/// (`[[source: xterm-reset]]` charproc.c 14548-14562).
#[test]
fn both_resets_clear_an_override_set_under_reverse() {
    each_read_size(b"\x1b[?5h\x1b]4;0;rgb:12/34/56\x07\x1b[!p", |t| {
        assert_eq!(overrides(t), Vec::<u8>::new());
        assert!(t.reverse_video(), "DECSTR keeps ?5");
    });
    each_read_size(b"\x1b[?5h\x1b]4;0;rgb:12/34/56\x07\x1bc", |t| {
        assert_eq!(overrides(t), Vec::<u8>::new());
        assert!(!t.reverse_video(), "RIS clears ?5");
    });
}

/// `CSI ? 996 n` answers the class of what `OSC 10`/`11` would answer, so on
/// a reversed dark theme it answers light (note Y, section 7: "Under `?5`
/// ... 996 follows it; no unsolicited report for `?5`").
#[test]
fn the_996_answer_follows_the_colours_shown() {
    each_read_size(b"\x1b[?996n", |t| assert_eq!(t.reply(), b"\x1b[?997;1n"));
    each_read_size(b"\x1b[?5h\x1b[?996n", |t| {
        assert_eq!(t.reply(), b"\x1b[?997;2n")
    });
    each_read_size(b"\x1b[?2031h\x1b[?5h\x1b[?5l\x1b[?5h", |t| {
        assert_eq!(t.reply(), b"", "?5 sends no notification")
    });
}

/// `OSC 12` names the cursor colour reversed or not: xterm's `oppositeColor`
/// leaves `TEXT_CURSOR` alone (`[[source: xterm-misc]]` 3527-3566).
#[test]
fn osc_12_is_not_mapped() {
    each_read_size(b"\x1b[?5h\x1b]12;rgb:12/34/56\x07\x1b]12;?\x07", |t| {
        assert_eq!(t.dynamic_color(DynamicColor::Cursor), Some(SET));
        assert_eq!(t.reply(), dynamic_reply(12, SET));
    });
}

/// One step of a program that flips `?5` among `OSC 10`/`11` sets and
/// queries. `OSC 4` is left out: its mapping waits on Jev's `DECSCNM`
/// decision.
#[derive(Clone, Debug)]
enum Op {
    Flip(bool),
    SetDynamic(u8, u8),
    QueryDynamic(u8),
}

fn op() -> impl proptest::strategy::Strategy<Value = Op> {
    use proptest::prelude::*;
    let code = 10u8..=11;
    prop_oneof![
        any::<bool>().prop_map(Op::Flip),
        (code.clone(), any::<u8>()).prop_map(|(c, v)| Op::SetDynamic(c, v)),
        code.prop_map(Op::QueryDynamic),
    ]
}

/// xterm's model, which **swaps what it stores** on every change of `?5`
/// (`ReverseVideo` and `ReverseOldColors`) and then sets and queries the
/// slot named, with no mapping -- against baud, which stores the unreversed
/// slots and maps the name. Resets are left out.
struct Xterm {
    video: Video,
    text: (u8, u8, u8),
    page: (u8, u8, u8),
}

impl Xterm {
    fn flip(&mut self, on: bool) {
        let to = if on { Video::Reverse } else { Video::Normal };
        if to != self.video {
            self.video = to;
            core::mem::swap(&mut self.text, &mut self.page);
        }
    }

    /// The bytes `op` is, and the reply xterm sends for them.
    fn step(&mut self, op: &Op) -> (Vec<u8>, Vec<u8>) {
        let colour = |v: u8| (v, v ^ 0x33, v ^ 0x66);
        match *op {
            Op::Flip(on) => {
                self.flip(on);
                let b = if on { "\x1b[?5h" } else { "\x1b[?5l" };
                (b.as_bytes().to_vec(), Vec::new())
            }
            Op::SetDynamic(code, v) => {
                match code {
                    10 => self.text = colour(v),
                    _ => self.page = colour(v),
                }
                let (r, g, b) = colour(v);
                let s = format!("\x1b]{code};rgb:{r:02x}/{g:02x}/{b:02x}\x07");
                (s.into_bytes(), Vec::new())
            }
            Op::QueryDynamic(code) => {
                let c = match code {
                    10 => self.text,
                    _ => self.page,
                };
                (
                    format!("\x1b]{code};?\x07").into_bytes(),
                    dynamic_reply(code, c),
                )
            }
        }
    }
}

proptest::proptest! {
    /// Every reply baud sends equals xterm's model's, at any read size.
    #[test]
    fn colours_under_flips_answer_as_xterm_does(
        ops in proptest::collection::vec(op(), 0..48),
        size in 1usize..=9,
    ) {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_default_colors(FG, BG);
        t.set_default_palette(&table());
        let mut model = Xterm { video: Video::Normal, text: FG, page: BG };
        for op in &ops {
            let (bytes, want) = model.step(op);
            for chunk in bytes.chunks(size) {
                support::feed(&mut t, chunk);
            }
            proptest::prop_assert_eq!(t.reply(), want.as_slice(), "{:?}", op);
            t.clear_reply();
        }
    }
}

/// **The selection's colours are named as sent under `?5` too**: `OSC 17`
/// sets, asks for and resets the highlight background and `OSC 19` the
/// foreground, as ctlseqs names them (Jev's lean, 0.84 and 0.72 under the
/// bar). The exchange is DEC's, which has no selection colour, and the
/// selection is drawn after it; xterm's handlers trade the two under its
/// own `reverseVideo` (misc.c `oppositeColor`), which leaves them where
/// they are. Unset, a query answers the selection as drawn: a default cell
/// exchanged and then reversed shows the text shown as its page.
#[test]
fn osc_17_and_19_under_reverse_name_the_colour_sent() {
    each_read_size(
        b"\x1b[?5h\x1b]17;rgb:12/34/56\x07\x1b]17;?\x07\x1b]19;?\x07",
        |t| {
            assert_eq!(
                t.highlight_colors(),
                SelectionColors {
                    bg: Some(SET),
                    fg: None
                }
            );
            assert_eq!(
                t.reply(),
                [dynamic_reply(17, SET), dynamic_reply(19, FG)].concat()
            );
        },
    );
    each_read_size(b"\x1b]19;rgb:12/34/56\x07\x1b[?5h\x1b]119\x07", |t| {
        assert_eq!(t.highlight_colors(), SelectionColors::default());
    });
}
