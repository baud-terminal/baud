//! **Every kind of failure the table compiler reports, made to fire** on a
//! small input written for it, with its position: a header that is not the
//! pinned file's, a code point that is not one, a range past U+10FFFF, the
//! wrong number of fields, a `First` without its `Last`, a value outside
//! the set, a duplicate, an overlap and a count missed, a sequence of code
//! points that is not one, two rows of a binary property over one code
//! point -- and, beside each,
//! the input that passes, so a check that cannot fail does not pass for one
//! that works.

use baud_tablegen::{
    CodepointMap, CpRange, Errors, Input, Kind, Pos, Source, Trie, ValueSet, binary_ranges,
};

const UCD: Input = Input {
    file: "x.txt",
    key: "x-key",
    header: "# X-17.0.0.txt",
};

fn src(text: &str) -> Source {
    Source::new(&UCD, text.to_string()).expect("the header is the pinned one")
}

#[test]
fn a_header_that_is_not_the_pinned_files_is_refused() {
    let e = Source::new(&UCD, "# X-16.0.0.txt\n0041; L\n".to_string()).expect_err("16.0.0");
    assert_eq!(e.kind, Kind::Header);
    assert_eq!(e.pos, Some(Pos { line: 1, col: 1 }));
    assert_eq!(
        e.to_string(),
        "data/x.txt:1:1: line 1 does not start with \"# X-17.0.0.txt\": \
         is this the pinned file [x-key]?: \"# X-16.0.0.txt\""
    );
    assert!(Source::new(&UCD, "# X-17.0.0.txt\n".to_string()).is_ok());
    let unread = Source::load(&UCD, &|_| Err(std::io::Error::other("gone"))).expect_err("unread");
    assert_eq!(unread.kind, Kind::Read);
}

#[test]
fn a_code_point_that_is_not_four_to_six_hex_digits_is_malformed() {
    let s = src("# X-17.0.0.txt\n00G0 ; L\n41 ; L\n0041..0030 ; L\n1234567 ; L\n0041..005A ; L\n");
    let rows: Vec<_> = s.rows().collect();
    for (i, want) in [
        (0, "\"00G0\""),
        (1, "\"41\""),
        (2, "runs backwards"),
        (3, "\"1234567\""),
    ] {
        let e = rows[i].range(0).expect_err("malformed");
        assert_eq!(e.kind, Kind::Malformed, "{e}");
        assert!(e.to_string().contains(want), "{e}");
    }
    assert_eq!(rows[4].range(0).ok(), Some(CpRange { lo: 0x41, hi: 0x5A }));
    // A single code point refuses a range.
    assert_eq!(
        rows[4].code_point(0).map_err(|e| e.kind),
        Err(Kind::Malformed)
    );
    let e = rows[0].range(0).expect_err("malformed");
    assert_eq!(e.pos, Some(Pos { line: 2, col: 1 }));
}

#[test]
fn a_range_past_u_10ffff_is_refused() {
    let s = src("# X-17.0.0.txt\n110000 ; L\n10FFFF..110000 ; L\n10FFFF ; L\n");
    let rows: Vec<_> = s.rows().collect();
    for r in &rows[..2] {
        assert_eq!(r.range(0).map_err(|e| e.kind), Err(Kind::PastLast));
    }
    assert_eq!(
        rows[2].range(0).ok(),
        Some(CpRange {
            lo: 0x10_FFFF,
            hi: 0x10_FFFF
        })
    );
}

#[test]
fn a_row_with_the_wrong_number_of_fields_is_refused_at_its_column() {
    let s = src("# X-17.0.0.txt\n0041 ; L ; extra # a comment; with a semicolon\n0042 ; L\n");
    let rows: Vec<_> = s.rows().collect();
    let e = rows[0].expect(2).expect_err("three fields");
    assert_eq!(e.kind, Kind::FieldCount);
    assert_eq!(e.pos, Some(Pos { line: 2, col: 12 }));
    assert!(e.to_string().contains("3 fields, 2 expected"), "{e}");
    assert!(rows[1].expect(2).is_ok());
    assert_eq!(rows[1].field(1), "L");
    assert_eq!(rows[1].get(1).map(|f| f.col), Some(8));
}

/// UAX #44 section 4.2.3: a range in UnicodeData.txt is a `<..., First>`
/// line and the `<..., Last>` line after it.
#[test]
fn a_first_without_its_last_is_refused() {
    const DATA: Input = Input {
        file: "ud.txt",
        key: "ud",
        header: "0000;",
    };
    let line = |cp: &str, name: &str| format!("{cp};{name};Lo;0;L;;;;;N;;;;;\n");
    let ud = |body: &[String]| Source::new(&DATA, body.concat()).expect("header");
    let good = ud(&[
        line("0000", "<control>"),
        line("4E00", "<CJK Ideograph, First>"),
        line("9FFF", "<CJK Ideograph, Last>"),
    ]);
    let entries: Vec<_> = good
        .unicode_data()
        .into_iter()
        .map(|e| e.map(|e| e.range))
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[1].as_ref().ok(),
        Some(&CpRange {
            lo: 0x4E00,
            hi: 0x9FFF
        })
    );
    let cases = [
        // A First, then an ordinary entry.
        vec![
            line("0000", "<control>"),
            line("4E00", "<CJK Ideograph, First>"),
            line("9FFF", "X"),
        ],
        // A Last with no First.
        vec![
            line("0000", "<control>"),
            line("9FFF", "<CJK Ideograph, Last>"),
        ],
        // A First, then another range's Last.
        vec![
            line("0000", "<control>"),
            line("4E00", "<CJK Ideograph, First>"),
            line("9FFF", "<Hangul Syllable, Last>"),
        ],
        // A First at the end of the file.
        vec![
            line("0000", "<control>"),
            line("4E00", "<CJK Ideograph, First>"),
        ],
    ];
    for body in cases {
        let s = ud(&body);
        let kinds: Vec<Kind> = s
            .unicode_data()
            .into_iter()
            .filter_map(Result::err)
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, [Kind::FirstWithoutLast], "{body:?}");
    }
    // UnicodeData.txt allows no spaces around a field, and has fifteen.
    let spaced = ud(&[
        line("0000", "<control>"),
        line("0041", " LATIN CAPITAL LETTER A"),
    ]);
    let kinds: Vec<Kind> = spaced
        .unicode_data()
        .into_iter()
        .filter_map(Result::err)
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, [Kind::Malformed]);
    let short = ud(&[line("0000", "<control>"), "0041;A;Lu\n".to_string()]);
    let kinds: Vec<Kind> = short
        .unicode_data()
        .into_iter()
        .filter_map(Result::err)
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, [Kind::FieldCount]);
}

const ALIASES: Input = Input {
    file: "pva.txt",
    key: "pva",
    header: "# PropertyValueAliases",
};

fn aliases(body: &str) -> Source {
    Source::new(
        &ALIASES,
        format!("# PropertyValueAliases-17.0.0.txt\n{body}"),
    )
    .expect("header")
}

#[test]
fn a_value_outside_the_set_is_refused_naming_where_the_set_is() {
    let pva =
        aliases("gc ; Lu ; Uppercase_Letter\nbc ; L ; Left_To_Right\nbc ; R ; Right_To_Left\n");
    let bc = ValueSet::aliases(&pva, "bc", &["R", "L"]).expect("the set");
    let s = src("# X-17.0.0.txt\n0041 ; L\n05D0 ; RTL\n# @missing: 0000..10FFFF; Right_To_Left\n");
    let rows: Vec<_> = s.rows().collect();
    assert_eq!(bc.check(&rows[0], 1).ok(), Some(1));
    let e = bc.check(&rows[1], 1).expect_err("RTL");
    assert_eq!(e.kind, Kind::UnknownValue);
    assert_eq!(e.pos, Some(Pos { line: 3, col: 8 }));
    assert!(
        e.to_string()
            .contains("bc `RTL` is not one of the 2 values of data/pva.txt:3-4"),
        "{e}"
    );
    let missing: Vec<_> = s.missing().collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].range(0).ok(),
        Some(CpRange {
            lo: 0,
            hi: 0x10_FFFF
        })
    );
    assert_eq!(bc.check_long(&missing[0], 1).ok(), Some(0));
    assert_eq!(
        bc.check(&missing[0], 1).map_err(|e| e.kind),
        Err(Kind::UnknownValue)
    );
    // The table's order must be the set exactly.
    let kinds = |order: &[&str]| {
        ValueSet::aliases(&pva, "bc", order)
            .map(|_| ())
            .map_err(|e| e.kinds())
    };
    assert_eq!(kinds(&["R"]), Err(vec![Kind::Invariant]));
    assert_eq!(kinds(&["R", "L", "AL"]), Err(vec![Kind::Invariant]));
    assert_eq!(kinds(&["L", "R"]), Ok(()));
    assert_eq!(
        ValueSet::aliases(&pva, "ea", &["W"])
            .map(|_| ())
            .map_err(|e| e.kinds()),
        Err(vec![Kind::Count])
    );
}

#[test]
fn a_key_given_twice_is_a_duplicate() {
    let pva = aliases("bc ; L ; Left_To_Right\nbc ; L ; Left_Again\n");
    let e = ValueSet::aliases(&pva, "bc", &["L"])
        .map(|_| ())
        .expect_err("twice");
    assert_eq!(e.kinds(), [Kind::Duplicate]);
    let s = src("# X-17.0.0.txt\n0028; 0029\n0028; 0029\n0029; 0028\n");
    let rows: Vec<_> = s.rows().collect();
    let mut map = CodepointMap::<u32>::new();
    let one = |r: &baud_tablegen::Row<'_>| {
        let cp = r.code_point(0).expect("a code point");
        CpRange { lo: cp, hi: cp }
    };
    assert!(map.assign(&s, &rows[0], one(&rows[0]), 0x29).is_ok());
    let e = map
        .assign(&s, &rows[1], one(&rows[1]), 0x29)
        .expect_err("twice");
    assert_eq!(e.kind, Kind::Duplicate);
    assert!(
        e.to_string()
            .contains("U+0028 is already set by line 2 (\"0028; 0029\")"),
        "{e}"
    );
    assert!(map.assign(&s, &rows[2], one(&rows[2]), 0x28).is_ok());
}

#[test]
fn two_rows_covering_one_code_point_overlap() {
    let s = src("# X-17.0.0.txt\n05EF..05F2 ; R\n05F0..05F4 ; AL\n05F3..05F4 ; AL\n");
    let rows: Vec<_> = s.rows().collect();
    let mut map = CodepointMap::<u8>::new();
    let range = |i: usize| rows[i].range(0).expect("a range");
    map.default_over(
        CpRange {
            lo: 0,
            hi: 0x10_FFFF,
        },
        9,
    );
    assert!(map.assign(&s, &rows[0], range(0), 1).is_ok());
    let e = map.assign(&s, &rows[1], range(1), 2).expect_err("U+05F0");
    assert_eq!(e.kind, Kind::Overlap);
    assert!(
        e.to_string()
            .starts_with("data/x.txt:3:1: U+05F0 is already set by line 2"),
        "{e}"
    );
    assert!(map.assign(&s, &rows[2], range(2), 2).is_ok());
    // Defaults layer under the data rows and never over them.
    map.default_over(
        CpRange {
            lo: 0x05EF,
            hi: 0x05FF,
        },
        7,
    );
    assert_eq!(map.get(0x05EF), Some(1));
    assert_eq!(map.get(0x05F5), Some(7));
    assert_eq!(map.get(0x0041), Some(9));
    assert_eq!(map.complete("x").map(|v| v.len()).ok(), Some(0x11_0000));
    let empty = CodepointMap::<u8>::new();
    assert_eq!(
        empty.complete("x").map_err(|e| e.kind).err(),
        Some(Kind::Invariant)
    );
}

#[test]
fn a_count_missed_is_reported_with_both_numbers() {
    let mut errs = Errors::default();
    errs.count("data/x.txt", "@missing lines", 24, 24);
    assert!(errs.is_empty());
    errs.count("data/x.txt", "@missing lines", 23, 24);
    assert_eq!(errs.kinds(), [Kind::Count]);
    assert_eq!(
        errs.to_string(),
        "data/x.txt: 23 @missing lines, 24 expected\n"
    );
    assert_eq!(
        errs.directives(),
        "cargo::error=data/x.txt: 23 @missing lines, 24 expected\n"
    );
    assert!(errs.or(()).is_err());
}

#[test]
fn a_trie_reads_back_its_values_and_refuses_a_stage_one_past_a_byte() {
    let values: Vec<u16> = (0..0x1000u16).map(|i| (i >> 8) % 3).collect();
    let t = Trie::build("T", &values, 8).expect("a trie");
    assert_eq!(t.distinct, 3);
    assert_eq!(t.index.len(), 16);
    // First-seen order: block 0 is the first one, all zeros.
    assert_eq!(&t.index[..4], [0, 1, 2, 0]);
    for (i, v) in values.iter().enumerate() {
        assert_eq!(t.get(i, 8), Some(*v));
    }
    // 257 distinct blocks of one value each do not fit a u8 stage 1.
    let wide: Vec<u16> = (0..257u16).flat_map(|b| [b, b]).collect();
    assert_eq!(
        Trie::build("W", &wide, 1).map(|_| ()).map_err(|e| e.kind),
        Err(Kind::Invariant)
    );
    let fits: Vec<u16> = (0..256u16).flat_map(|b| [b, b]).collect();
    assert_eq!(
        Trie::build("F", &fits, 1).map(|t| t.distinct).ok(),
        Some(256)
    );
    assert_eq!(
        Trie::build("P", &[1u8; 3], 1)
            .map(|_| ())
            .map_err(|e| e.kind),
        Err(Kind::Invariant)
    );
}

#[test]
fn a_sequence_of_code_points_is_each_four_to_six_hex_digits() {
    let s = src("# X-17.0.0.txt\n00C5;<final> 0041 030A\n00C6;0041 30A\n00C7;<final>\n");
    let rows: Vec<_> = s.rows().collect();
    assert_eq!(
        rows[0].code_points(1, "<final>".len()).expect("two"),
        [0x41, 0x30A]
    );
    let e = rows[1].code_points(1, 0).expect_err("30A");
    assert_eq!(e.kind, Kind::Malformed);
    assert_eq!(e.pos, Some(Pos { line: 3, col: 6 }));
    assert!(e.to_string().contains("\"30A\" is not a code point"), "{e}");
    let e = rows[2].code_points(1, "<final>".len()).expect_err("none");
    assert_eq!(
        e.to_string(),
        "data/x.txt:4:6: no code points: \"00C7;<final>\""
    );
}

#[test]
fn a_binary_property_is_its_rows_merged_and_an_overlap_is_refused() {
    let s = src("# X-17.0.0.txt\n0041..0042 ; P\n0030 ; Q\n0043 ; P\n0045 ; P\n0020 ; P\n");
    let r = binary_ranges(&s, "P").expect("no overlap");
    assert_eq!(
        r,
        [
            CpRange { lo: 0x20, hi: 0x20 },
            CpRange { lo: 0x41, hi: 0x43 },
            CpRange { lo: 0x45, hi: 0x45 },
        ]
    );
    let s = src("# X-17.0.0.txt\n0041..0043 ; P\n0042 ; P\n0050 ; P ; extra\n");
    let e = binary_ranges(&s, "P").expect_err("an overlap and a field too many");
    assert_eq!(e.kinds(), [Kind::FieldCount, Kind::Overlap]);
    assert!(
        e.list()[1]
            .to_string()
            .starts_with("data/x.txt:3:1: U+0042 is already given P by the rows up to line 2"),
        "{e}"
    );
}
