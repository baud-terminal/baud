//! The six 96-character sets, the designations DECCIR reports, and DECCIR's
//! data in both directions.
//!
//! `build/iso96.rs` compiles the tables from the pinned mapping files; the
//! tests here hold them to cells read off the files by hand.

use baud_domain::*;

fn sb(b: u8) -> StreamByte {
    StreamByte::from_stream(b)
}

/// Cells read off each pinned file: 2/0 is NO-BREAK SPACE in every set, and
/// each set's own letters sit where its ISO 8859 part puts them. Greek's
/// 0xA4, which only the 2003 edition assigns, and Hebrew's 0xFD, the 1999
/// edition's LRM, are unassigned in the registered sets.
#[test]
fn each_96_set_draws_its_iso_8859_right_half() {
    let at = |s: Iso96, code: u8| s.table()[usize::from(code - 0x20)].value();
    for s in [
        Iso96::Latin1,
        Iso96::Latin2,
        Iso96::Greek,
        Iso96::Hebrew,
        Iso96::LatinCyrillic,
        Iso96::Latin5,
    ] {
        assert_eq!(at(s, 0x20), 0x00a0, "{s:?} 2/0");
        assert_eq!(Iso96::from_u8(s.to_u8()), Some(s));
    }
    assert_eq!(at(Iso96::Latin1, 0x7f), 0x00ff);
    assert_eq!(at(Iso96::Latin2, 0x21), 0x0104);
    assert_eq!(at(Iso96::Greek, 0x61), 0x03b1);
    assert_eq!(at(Iso96::Greek, 0x24), DEC_ERROR_CHARACTER.value());
    assert_eq!(at(Iso96::Hebrew, 0x60), 0x05d0);
    assert_eq!(at(Iso96::Hebrew, 0x7d), DEC_ERROR_CHARACTER.value());
    assert_eq!(at(Iso96::LatinCyrillic, 0x30), 0x0410);
    assert_eq!(at(Iso96::Latin5, 0x5d), 0x0130);
}

/// The 96-series finals are their own series: `A` is ISO Latin-1
/// Supplemental there and the UK set among the 94.
#[test]
fn a_designation_names_its_set_by_its_series() {
    assert_eq!(Iso96::from_final(sb(b'A')), Some(Iso96::Latin1));
    assert_eq!(Iso96::from_final(sb(b'Z')), None);
    assert_eq!(
        Designation::of_96(sb(b'A')).charset(),
        Charset::Iso96(Iso96::Latin1)
    );
    assert_eq!(Designation::of_94(None, sb(b'A')).charset(), Charset::Uk);
    assert_eq!(Designation::of_96(sb(b'Z')).charset(), Charset::Other);
    let d = Designation::of_94(Some(sb(b'%')), sb(b'5'));
    assert_eq!(d.tail(), b"%5");
    assert_eq!(d.size(), SetSize::Chars94);
    assert_eq!(d.charset(), Charset::Dec(DecSet::SupplementalGraphics));
    assert_eq!(Designation::UPSS.tail(), b"<");
    assert_eq!(Designation::ASCII.charset(), Charset::Ascii);
    assert!(Charset::Iso96(Iso96::Greek).is_96());
    assert!(!Charset::Dec(DecSet::Supplemental).is_96());
    assert_eq!(GSlot::from_intermediate96(sb(b'.')), Some(GSlot::G2));
    assert_eq!(GSlot::from_intermediate96(sb(b'(')), None);
    assert_eq!(GSlot::from_index(3), Some(GSlot::G3));
    assert_eq!(GSlot::from_index(4), None);
}

/// A 96-set's character carries its set's tag, and the tag reads back.
#[test]
fn a_96_set_has_a_tag_of_its_own() {
    let r = Received::of(Charset::Iso96(Iso96::Hebrew)).expect("tagged");
    assert_eq!(Received::from_tag(r.tag()), Some(r));
    let f = CellFlags::NONE.with_received(Some(r));
    assert_eq!(Charset::from_flags(f), Charset::Iso96(Iso96::Hebrew));
    // The tag before the line-drawing flag: a GR write while GL held line
    // drawing reads back as the set it was received under.
    let g = CellFlags::GRAPHICS.with_received(Some(r));
    assert_eq!(Charset::from_flags(g), Charset::Iso96(Iso96::Hebrew));
    assert_eq!(
        Charset::from_flags(CellFlags::GRAPHICS),
        Charset::DecGraphics
    );
}

fn ws() -> (Rows, Cols) {
    (Rows::of_count(24), Cols::of_index(80))
}

/// DECCIR's data written and read back field for field.
#[test]
fn cursor_information_round_trips() {
    let ci = CursorInformation {
        row: Row::from_index(4).one_based(),
        col: Col::at_index(79).one_based(),
        rendition: CirBits::NONE.with(CirBits::BIT1, ModeChange::Set),
        attribute: CirBits::BIT1,
        flags: CirBits::NONE.with(CirBits::BIT4, ModeChange::Set),
        gl: GSlot::G1,
        gr: GSlot::G3,
        designations: [
            Designation::ASCII,
            Designation::of_94(None, sb(b'0')),
            Designation::of_96(sb(b'A')),
            Designation::of_94(Some(sb(b'%')), sb(b'5')),
        ],
    };
    let mut out = [0u8; CIR_DATA_MAX];
    let n = ci.encode(&mut out);
    assert_eq!(&out[..n], b"5;80;1;A;A;H;1;3;D;B0A%5");
    assert!(ci.sizes().has(CirBits::BIT3));
    let (rows, cols) = ws();
    let r = CirRestore::parse(&out[..n], rows, cols);
    assert_eq!(r.row, Some(ci.row));
    assert_eq!(r.col, Some(ci.col));
    assert_eq!(r.rendition, Some(ci.rendition));
    assert_eq!(r.attribute, Some(ci.attribute));
    assert_eq!(r.flags, Some(ci.flags));
    assert_eq!((r.gl, r.gr), (Some(GSlot::G1), Some(GSlot::G3)));
    assert_eq!(r.designations, ci.designations.map(Some));
}

/// A restore stops at its first invalid value: a row past the screen, a page
/// other than 1, a flag byte not of the `0x40` form, a 96-set in G0.
#[test]
fn a_restore_stops_at_the_first_invalid_value() {
    let (rows, cols) = ws();
    assert_eq!(
        CirRestore::parse(b"25;1;1;@;@;@;0;2;@;BB<<", rows, cols),
        CirRestore::default()
    );
    let r = CirRestore::parse(b"5;7;2;C;@;@;0;2;@;BB<<", rows, cols);
    assert_eq!(
        (r.row.map(|v| v.get()), r.col.map(|v| v.get()), r.rendition),
        (Some(5), Some(7), None)
    );
    let r = CirRestore::parse(b"5;7;1;\x7f;@;@;0;2;@;BB<<", rows, cols);
    assert_eq!(r.rendition, None);
    let r = CirRestore::parse(b"1;1;1;@;@;@;0;2;A;AB<<", rows, cols);
    assert_eq!((r.gr, r.designations[0]), (Some(GSlot::G2), None));
    assert_eq!(
        PresentationState::from_param(1),
        Some(PresentationState::CursorInformation)
    );
    assert_eq!(
        PresentationState::from_param(2),
        Some(PresentationState::TabStops)
    );
    assert_eq!(PresentationState::from_param(0), None);
    assert_eq!(CirBits::BIT2.wire(), b'B');
}

/// DECTABSR written whole or not at all, and read back a column at a time.
#[test]
fn a_tab_report_is_whole_or_nothing_and_reads_back() {
    let mut tabs = [false; 20];
    tabs[4] = true;
    tabs[12] = true;
    let mut out = [0u8; 64];
    let n = encode_tab_report(&tabs, C1Transmit::SevenBit, &mut out).expect("fits");
    assert_eq!(&out[..n.index()], b"\x1bP2$u5/13\x1b\\");
    let mut small = [0u8; 8];
    assert!(encode_tab_report(&tabs, C1Transmit::SevenBit, &mut small).is_none());
    assert_eq!(tab_report_max(9), 2 + 3 + 2 + 9 + 8);
    let mut st = TabRestore::START;
    let mut got = Vec::new();
    for &b in b"5/13" {
        st = match st.put(sb(b)) {
            TabRestore::Ended(c) => {
                got.push(c.get());
                TabRestore::START
            }
            next => next,
        };
    }
    got.extend(st.last().map(|c| c.get()));
    assert_eq!(got, [5, 13]);
    assert_eq!(
        TabRestore::START.put(sb(b'/')),
        TabRestore::Stopped,
        "a column of 0"
    );
    assert_eq!(TabRestore::START.put(sb(b'x')), TabRestore::Stopped);
}
