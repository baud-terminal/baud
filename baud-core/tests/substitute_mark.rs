#![deny(dead_code)]
//! **`SUB` draws the error character when the embedder turns the mark on**,
//! and nothing otherwise, the owner's call of 2026-10-23 ("i would say keep
//! A as default and add config option and maybe also which char to show?
//! and default is the reverse question mark").
//!
//! Where and how the mark is written is DEC's: `[[source: dec-std-070]]` p.
//! 5-132, SUB "is processed by placing a reversed question mark in the
//! display at the Active Position", by `insert_or_replace_character()`, the
//! routine every graphic character is written by (pp. 5-140-5-141): in the
//! current rendition and DECSCA attribute, shifting the line under IRM, the
//! cursor moving on and wrapping under DECAWM; and note 2, within an escape
//! or control sequence "the sequence in progress to be aborted and the
//! Substitute control function to be executed instead". VT420 and VT510:
//! SUB "Immediately cancels an escape sequence, control sequence, or device
//! control string in progress, and displays a reverse question mark". A
//! Sixel's body keeps 070-9 §9.1's blank sixel, and VT52 mode draws nothing
//! (Jev's lean, ANSI_MODE_ONLY, 0.76 and 0.84, which a re-ask split).

use baud_core::*;
use baud_domain::{ErrorCharacter, SubstituteMark, UnfitMark};

const ERR: u32 = 0x2426;

/// A terminal with `mark`, fed `bytes` in reads of `size`, handed to `f`.
fn fed<R>(mark: SubstituteMark, bytes: &[u8], size: usize, f: impl FnOnce(&Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 20,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.set_substitute_mark(mark);
    for part in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, part);
        t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
    }
    f(&t)
}

/// Every read size, from one byte to the whole.
fn each_size(mark: SubstituteMark, bytes: &[u8], f: impl Fn(&Term<'_>, usize)) {
    for size in 1..=bytes.len() {
        fed(mark, bytes, size, |t| f(t, size));
    }
}

const ON: SubstituteMark = SubstituteMark::Drawn(ErrorCharacter::DEC);

/// Row `y`'s first `n` cells' bases, `None` for blank and for the base a
/// lone mark is stored on.
fn bases(t: &Term<'_>, y: usize, n: usize) -> Vec<Option<u32>> {
    let row = t.row(Row::from_index(y)).expect("a row");
    (0..n)
        .map(|x| {
            let c = row[x];
            match c.content.is_empty() {
                true => None,
                false => t.base_of(c).as_codepoint().map(|b| b.value()),
            }
        })
        .collect()
}

fn ch(c: char) -> Option<u32> {
    Some(u32::from(c))
}

/// **Off, the default, a `SUB` writes nothing wherever it arrives; on, it
/// writes the mark at the cursor and the cursor moves past it**: in the
/// ground, and cancelling an escape sequence, a control sequence, a single
/// shift, an `OSC`, a DECRQSS and an `APC`. `a` before, `b` after, every
/// read size. Perturbation: give `SUB` `Consume` in the cancel edge -- every
/// case but the ground's loses its mark.
#[test]
fn a_sub_draws_the_mark_where_it_arrives_only_when_turned_on() {
    let cases: [(&str, &[u8]); 8] = [
        ("ground", b"a\x1ab"),
        ("escape", b"a\x1b(\x1ab"),
        ("csi", b"a\x1b[1;5\x1ab"),
        ("csi intermediate", b"a\x1b[1 \x1ab"),
        ("single shift", b"a\x1bN\x1ab"),
        ("osc", b"a\x1b]2;title\x1ab"),
        ("decrqss", b"a\x1bP$qm\x1ab"),
        ("apc", b"a\x1b_xyz\x1ab"),
    ];
    for (name, bytes) in cases {
        assert_eq!(SubstituteMark::default(), SubstituteMark::Off);
        each_size(SubstituteMark::Off, bytes, |t, size| {
            assert_eq!(
                bases(t, 0, 3),
                [ch('a'), ch('b'), None],
                "{name} off at {size}"
            );
            assert_eq!(
                t.cursor(),
                (Row::from_index(0), Col::at_index(2)),
                "{name} {size}"
            );
        });
        each_size(ON, bytes, |t, size| {
            assert_eq!(
                bases(t, 0, 4),
                [ch('a'), Some(ERR), ch('b'), None],
                "{name} on at {size}"
            );
            assert_eq!(
                t.cursor(),
                (Row::from_index(0), Col::at_index(3)),
                "{name} {size}"
            );
            assert!(t.title().is_empty(), "{name}: a cancelled title is not set");
            assert!(
                t.reply().is_empty(),
                "{name}: a cancelled request is not answered"
            );
        });
    }
}

/// **The mark is written as a graphic character is**: in the current
/// rendition (bold red, as the `x` after it), protected under DECSCA so a
/// selective erase spares it, shifting the line right under IRM, and
/// wrapping at the margin under DECAWM, the next character on the line
/// below. Every read size.
#[test]
fn the_mark_takes_the_rendition_protection_insert_and_wrap_of_a_character() {
    each_size(ON, b"\x1b[1;31m\x1ax\x1b[m", |t, size| {
        let row = t.row(Row::from_index(0)).expect("a row");
        assert_eq!(
            row[0].style, row[1].style,
            "the mark's rendition is x's, at {size}"
        );
        assert_ne!(
            t.style_of(row[0]),
            t.style_of(row[2]),
            "and not the default, at {size}"
        );
    });
    each_size(ON, b"\x1b[1\"q\x1a\x1b[0\"qz\x1b[?2K", |t, size| {
        let b = bases(t, 0, 2);
        assert_eq!(b[0], Some(ERR), "DECSEL spares the mark, at {size}");
        assert_ne!(b[1], ch('z'), "the control: it erases the z, at {size}");
        let row = t.row(Row::from_index(0)).expect("a row");
        assert!(row[0].is_protected(), "at {size}");
    });
    each_size(ON, b"abc\r\x1b[4h\x1a", |t, size| {
        assert_eq!(
            bases(t, 0, 5),
            [Some(ERR), ch('a'), ch('b'), ch('c'), None],
            "IRM, at {size}"
        );
    });
    let mut wrap = b"xxxxxxxxxxxxxxxxxxx".to_vec();
    wrap.extend_from_slice(b"\x1ay");
    each_size(ON, &wrap, |t, size| {
        assert_eq!(bases(t, 0, 20)[19], Some(ERR), "the last column, at {size}");
        assert_eq!(bases(t, 1, 2), [ch('y'), None], "wrapped, at {size}");
    });
}

/// **The mark is a cluster of its own, before and after**: `SUB` is a
/// control (`[[source: uax29]]` GB4, GB5), so U+0301 after it takes a cell
/// alone, and the mark does not join the `e` before it. After a Prepend,
/// U+0600, which GB9b joins to what follows, the row is the row of a `BEL`
/// and the character typed, cell for cell. Every read size. Perturbation:
/// leave the break out before the mark -- it joins the U+0600.
#[test]
fn the_mark_is_a_cluster_of_its_own() {
    let typed = fed(
        SubstituteMark::Off,
        "a\u{600}\x07\u{2426}b".as_bytes(),
        64,
        |t| t.row(Row::from_index(0)).expect("a row")[..5].to_vec(),
    );
    each_size(ON, "a\u{600}\x1ab".as_bytes(), |t, size| {
        let row = t.row(Row::from_index(0)).expect("a row");
        assert_eq!(row[..5], typed[..], "after a Prepend, at {size}");
    });
    each_size(ON, "e\x1a\u{301}".as_bytes(), |t, size| {
        assert_eq!(bases(t, 0, 3)[..2], [ch('e'), Some(ERR)], "at {size}");
        let row = t.row(Row::from_index(0)).expect("a row");
        assert_eq!(
            t.base_of(row[2]),
            Content::DEFECTIVE,
            "the accent alone, at {size}"
        );
        assert_eq!(
            t.marks(Row::from_index(0), Col::at_index(1)).count(),
            0,
            "at {size}"
        );
    });
}

/// **A `SUB` cutting a character short writes the replacement and then the
/// mark**: the partial character becomes U+FFFD and the `SUB` is read from
/// the ground (`[[source: unicode-ch3]]` 3.9.5). Every read size.
#[test]
fn a_sub_inside_a_character_writes_the_replacement_then_the_mark() {
    each_size(ON, b"a\xe2\x86\x1ab", |t, size| {
        assert_eq!(
            bases(t, 0, 5),
            [ch('a'), Some(0xfffd), Some(ERR), ch('b'), None],
            "at {size}"
        );
    });
}

/// **A Sixel's body keeps its blank sixel and draws no mark; a `SUB` after
/// the body's `ESC` cancels that escape sequence and draws one**: 070-9
/// §9.1, "SUB - process as if a 3/15 code (blank Sixel) was received", and
/// the `ESC` ends the Sixel and begins an escape sequence, which the `SUB`
/// then cancels as any other. With no image memory the Sixel is refused
/// and the cells are text's alone. Every read size.
#[test]
fn a_sixel_body_keeps_its_blank_sixel_and_its_escape_takes_the_mark() {
    each_size(ON, b"\x1bPq~\x1a~\x1b\\b", |t, size| {
        assert_eq!(bases(t, 0, 2), [ch('b'), None], "a blank sixel, at {size}");
    });
    each_size(ON, b"\x1bPq~\x1b\x1ab", |t, size| {
        assert_eq!(bases(t, 0, 3), [Some(ERR), ch('b'), None], "at {size}");
    });
}

/// **`CAN` draws nothing, and VT52 mode draws nothing for `SUB`**:
/// VT220's CAN, "No error character is displayed"; and Jev's lean,
/// ANSI_MODE_ONLY, for VT52, in its ground and inside `ESC Y`. Every read
/// size. Perturbation: draw it in VT52 ground -- the second case fails.
#[test]
fn can_and_vt52_mode_draw_no_mark() {
    each_size(ON, b"a\x1b[1\x18b", |t, size| {
        assert_eq!(bases(t, 0, 3), [ch('a'), ch('b'), None], "CAN at {size}");
    });
    each_size(ON, b"\x1b[?2la\x1ab\x1bY\x1ac", |t, size| {
        assert_eq!(
            bases(t, 0, 4),
            [ch('a'), ch('b'), ch('c'), None],
            "VT52 at {size}"
        );
    });
}

/// **The character is the embedder's, and resets keep the setting**: a
/// `?` drawn in U+2426's place, and after `RIS` and `DECSTR` the mark is
/// still drawn. A character that is not one cell of its own is refused.
#[test]
fn the_mark_is_the_embedders_character_and_resets_keep_it() {
    let q = ErrorCharacter::of(Codepoint::from_char('?')).expect("one cell");
    let mark = SubstituteMark::Drawn(q);
    each_size(mark, b"\x1a\x1bc\x1a\x1b[!p\x1a", |t, size| {
        assert_eq!(bases(t, 0, 3)[0], ch('?'), "at {size}");
        assert_eq!(t.substitute_mark(), mark, "at {size}");
    });
    fed(mark, b"\x1bc\x1b[!pa\x1a", 64, |t| {
        assert_eq!(bases(t, 0, 3), [ch('a'), ch('?'), None]);
    });
    assert_eq!(
        ErrorCharacter::of(Codepoint::from_char('\u{301}')),
        Err(UnfitMark::NotAlone)
    );
}
