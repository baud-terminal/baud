//! **`[[source: unicode-ch3]]` Table 3-7, as the decoder's tables hold it.**
//!
//! "Table 3-7. Well-Formed UTF-8 Byte Sequences", as printed:
//!
//! ```text
//! U+0000..U+007F     00..7F
//! U+0080..U+07FF     C2..DF  80..BF
//! U+0800..U+0FFF     E0      A0..BF  80..BF
//! U+1000..U+CFFF     E1..EC  80..BF  80..BF
//! U+D000..U+D7FF     ED      80..9F  80..BF
//! U+E000..U+FFFF     EE..EF  80..BF  80..BF
//! U+10000..U+3FFFF   F0      90..BF  80..BF  80..BF
//! U+40000..U+FFFFF   F1..F3  80..BF  80..BF  80..BF
//! U+100000..U+10FFFF F4      80..8F  80..BF  80..BF
//! ```
//!
//! "These exceptions to the general pattern occur only in the second byte of
//! a sequence", and "the following byte values are disallowed in UTF-8:
//! C0–C1, F5–FF". The rows are written out below a second time, from the
//! text and not from `decode.rs`, so the tables answer to the standard and
//! not to themselves; and `std::str::from_utf8` is a third, independent
//! reading.

use baud_domain::*;

/// The rows above: first-byte range, second-byte range, length.
const ROWS: [(u8, u8, u8, u8, usize); 8] = [
    (0xc2, 0xdf, 0x80, 0xbf, 2),
    (0xe0, 0xe0, 0xa0, 0xbf, 3),
    (0xe1, 0xec, 0x80, 0xbf, 3),
    (0xed, 0xed, 0x80, 0x9f, 3),
    (0xee, 0xef, 0x80, 0xbf, 3),
    (0xf0, 0xf0, 0x90, 0xbf, 4),
    (0xf1, 0xf3, 0x80, 0xbf, 4),
    (0xf4, 0xf4, 0x80, 0x8f, 4),
];

fn row(lead: u8) -> Option<(u8, u8, usize)> {
    ROWS.iter()
        .find(|r| (r.0..=r.1).contains(&lead))
        .map(|r| (r.2, r.3, r.4))
}

/// **The second-byte table is Table 3-7's second column**, on all 65,536
/// pairs: a byte leading a row admits exactly that row's range, and any
/// other byte admits nothing.
#[test]
fn the_second_byte_table_is_table_3_7() {
    let mut admitted = 0usize;
    for lead in 0u8..=0xff {
        for second in 0u8..=0xff {
            let want = row(lead).is_some_and(|(lo, hi, _)| (lo..=hi).contains(&second));
            let got = StreamByte::from_stream(lead).admits_second(StreamByte::from_stream(second));
            assert_eq!(got, want, "{lead:#04x} then {second:#04x}");
            admitted += usize::from(got);
        }
    }
    // Positive control: 30 two-byte leads and 16 three-byte and 5 four-byte,
    // with the four narrowed rows' 32, 32, 48 and 16 second bytes.
    assert_eq!(admitted, 30 * 64 + 14 * 64 + 32 + 32 + 3 * 64 + 48 + 16);
}

/// **The width table is Table 3-7's first column**: a row's length for its
/// leads, 0 for a continuation, and 1 for ASCII and for the disallowed
/// `C0-C1` and `F5-FF`, each a maximal subpart of one byte (3.9.6, D93b).
#[test]
fn the_width_table_is_table_3_7() {
    for b in 0u8..=0xff {
        let want = match (b, row(b)) {
            (0x80..=0xbf, _) => 0,
            (_, Some((_, _, n))) => n,
            _ => 1,
        };
        assert_eq!(StreamByte::from_stream(b).utf8_width(), want, "{b:#04x}");
    }
    for b in [0xc0u8, 0xc1, 0xf5, 0xf8, 0xff] {
        assert_eq!(StreamByte::from_stream(b).utf8_width(), 1, "{b:#04x}");
    }
}

/// **For a lead Table 3-7 allows followed by continuations, the decode
/// refuses exactly what the second byte refuses.** The run writer finds a
/// run by shape and ends it where the decode -- the shortest-form floor,
/// the surrogate hole, the U+10FFFF ceiling -- refuses a character, and the
/// one-character path asks `admits_second`; the two are one rule only if
/// this holds, so it is checked on every sequence of every length rather
/// than argued: 1,920 two-byte, 65,536 three-byte and 1,310,720 four-byte.
/// `from_utf8` is the third opinion.
#[test]
fn a_valid_lead_decodes_to_a_character_iff_it_admits_its_second_byte() {
    let decode = |s: &[u8]| -> Option<Codepoint> {
        let b: Vec<StreamByte> = s.iter().map(|b| StreamByte::from_stream(*b)).collect();
        let mut raw = b[0].lead_bits(s.len());
        for c in &b[1..] {
            raw = (raw << 6) | c.continuation_bits();
        }
        let floor = match s.len() {
            2 => 0x80,
            3 => 0x800,
            _ => 0x1_0000,
        };
        (raw >= floor).then(|| Codepoint::new(raw)).flatten()
    };
    let conts = 0x80u8..=0xbf;
    let mut seen = [0usize; 5];
    for lead in 0xc2u8..=0xf4 {
        let (_, _, n) = row(lead).expect("a lead");
        let lead_b = StreamByte::from_stream(lead);
        for second in conts.clone() {
            let admits = lead_b.admits_second(StreamByte::from_stream(second));
            let mut tails: Vec<Vec<u8>> = vec![vec![]];
            for _ in 2..n {
                tails = tails
                    .into_iter()
                    .flat_map(|t| {
                        conts.clone().map(move |c| {
                            let mut t = t.clone();
                            t.push(c);
                            t
                        })
                    })
                    .collect();
            }
            for tail in tails {
                let s = [&[lead, second][..], &tail].concat();
                let got = decode(&s);
                assert_eq!(got.is_some(), admits, "{s:02x?}");
                assert_eq!(core::str::from_utf8(&s).is_ok(), admits, "{s:02x?}");
                if n == 3 {
                    let v = Codepoint::from_three_bytes(
                        usize::from(s[0]),
                        usize::from(s[1]),
                        usize::from(s[2]),
                    );
                    assert_eq!(Codepoint::from_three(v), got, "{s:02x?}");
                }
                seen[n] += 1;
            }
        }
    }
    assert_eq!(seen, [0, 0, 30 * 64, 16 * 64 * 64, 5 * 64 * 64 * 64]);
}
