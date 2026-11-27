//! The level table, `baud_domain::LEVELS`, read against DEC STD 070.
//!
//! `std070_levels.tsv` is every `Levels:` field of `[[source: dec-std-070]]`,
//! transcribed from the pinned copy as the manifest's comment on it says,
//! its header naming that copy's sha256.
//! Each row of the table that cites a field must give the lowest level the
//! field names; each field must belong to a row or to the list here of
//! functions baud does not dispatch; and each row must be reached by the
//! parsed forms that perform it, so a control dispatched under the wrong
//! function, or under none, fails.

use baud_domain::*;
use std::collections::{BTreeMap, BTreeSet};

/// The transcription: mnemonic to (PDF page, field).
fn fields() -> BTreeMap<String, (u16, String)> {
    let src = include_str!("std070_levels.tsv");
    let mut out = BTreeMap::new();
    for line in src.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let mut it = line.split('\t');
        let page: u16 = it.next().unwrap().parse().unwrap();
        let mnemonic = it.next().unwrap().to_string();
        let field = it.next().unwrap().to_string();
        assert!(
            out.insert(mnemonic.clone(), (page, field)).is_none(),
            "{mnemonic} twice"
        );
    }
    out
}

/// **The transcription names the copy it was read from**: its `#` header
/// carries `[[source: dec-std-070]]`'s sha256 as the manifest records it, so
/// a pin moved to another copy leaves the transcription stale, and red here.
#[test]
fn the_transcription_names_the_pinned_copys_sha256() {
    let manifest = include_str!("../../../docs/research/sources/MANIFEST.toml");
    let header: String = include_str!("std070_levels.tsv")
        .lines()
        .take_while(|l| l.starts_with('#'))
        .collect();
    let sha = pinned_sha256(manifest, "dec-std-070");
    assert!(header.contains("[[source: dec-std-070]]"), "{header}");
    assert!(header.contains(&format!("sha256 {sha}")), "{header}");
}

/// The `sha256` of the manifest's entry `key`, read with no TOML parser:
/// the line after `[sources.<key>]` that starts `sha256`.
fn pinned_sha256(manifest: &str, key: &str) -> String {
    let head = format!("[sources.{key}]");
    let at = manifest.find(&head).unwrap_or_else(|| panic!("no {head}"));
    let line = manifest[at..]
        .lines()
        .find(|l| l.starts_with("sha256"))
        .expect("a sha256 line");
    let sha = line.split('"').nth(1).expect("a quoted sha256");
    assert_eq!(sha.len(), 64, "{line}");
    sha.to_string()
}

/// The lowest level a field names, or `None` for a documented exception.
fn lowest(field: &str) -> Option<ConformanceLevel> {
    match field.chars().find(|c| c.is_ascii_digit())? {
        '1' => Some(ConformanceLevel::One),
        '2' => Some(ConformanceLevel::Two),
        '3' => Some(ConformanceLevel::Three),
        '4' => Some(ConformanceLevel::Four),
        c => panic!("level {c} in {field:?}"),
    }
}

/// **The positive control**: three fields read on their page images come
/// out of the transcription as printed, so a field it misses is missing.
#[test]
fn the_transcription_reads_the_fields_its_pages_show() {
    let f = fields();
    assert_eq!(f["CUU"], (296, "1-4".to_string()));
    assert_eq!(f["DECID"], (232, "1, 2".to_string()));
    assert_eq!(f["DECSSDT"], (1048, "2x, 3, 4".to_string()));
    assert_eq!(f.len(), 140);
}

/// Fields of functions baud does not dispatch, each with why: the reports a
/// terminal sends, and the functions it does not build.
const NOT_DISPATCHED: &[(&str, &str)] = &[
    ("DA2", "unbuilt: CSI > c is counted (the DA2 row)"),
    ("DA3", "unbuilt: CSI = c is counted"),
    ("DECSR", "unbuilt: CSI + p is counted"),
    ("DECHCCM", "no pages to couple"),
    ("DECVCCM", "no pages to couple"),
    ("DECPCCM", "no pages to couple"),
    ("DECSNLS", "a documented exception, unbuilt"),
    ("DECSTGLT", "a documented exception, unbuilt"),
    ("IRGC", "the write itself, not a control"),
    (
        "DECCTR",
        "DECRSTS 2's and DECRQTSR 2's body, gated with them",
    ),
    (
        "DECNRCM (keyboard)",
        "unbuilt: the keyboard's 7-bit NRC codes, with no keyboard dialect",
    ),
    ("DECKBUM", "unbuilt"),
    ("DECDMAC", "unbuilt"),
    ("DECINVM", "unbuilt"),
    ("DECUDK", "unbuilt (its row)"),
    ("DECLRP", "a report baud sends"),
    ("DECRPM", "a report baud sends, DECRQM's"),
    ("DECRPSS", "a report baud sends, DECRQSS's"),
    ("DECPSR", "a report baud sends, DECRQPSR's"),
    ("DECCIR", "a report baud sends, DECRQPSR 1's"),
    ("DECTABSR", "a report baud sends, DECRQPSR 2's"),
    ("DECTSR", "a report baud sends, DECRQTSR's"),
];

/// A field whose lowest level another passage of the text overrides, with
/// the level the table gives.
const OVERRIDES: &[(&str, ConformanceLevel, &str)] = &[(
    "ACS",
    ConformanceLevel::Three,
    "its algorithm: 'if (conformance_level > 2)'",
)];

/// **Every row that cites a field gives the field's lowest level**, at the
/// field's page; an overridden one the level its passage gives.
#[test]
fn every_row_is_its_fields_lowest_level() {
    let f = fields();
    let mut wrong = Vec::new();
    for r in LEVELS {
        let got = r.introduced.least();
        match r.source {
            LevelSource::Field(page) => match f.get(r.mnemonic) {
                Some((p, field)) if *p == page => {
                    if lowest(field) != Some(got) {
                        wrong.push(format!("{} is {got:?}, its field {field:?}", r.mnemonic));
                    }
                }
                other => wrong.push(format!(
                    "{} cites PDF {page}, the transcription {other:?}",
                    r.mnemonic
                )),
            },
            LevelSource::Overridden(page) => {
                assert_eq!(f.get(r.mnemonic).map(|x| x.0), Some(page), "{}", r.mnemonic);
                let want = OVERRIDES.iter().find(|o| o.0 == r.mnemonic);
                match want {
                    Some((_, l, _)) if *l == got => {}
                    _ => wrong.push(format!(
                        "{} overrides its field with no listed reason",
                        r.mnemonic
                    )),
                }
            }
            LevelSource::Extension(n) => {
                // 070-4 Table 1 (PDF 223): 3 ReGIS Display and 4 Sixels
                // Display, "1-4".
                assert!(matches!(n, 3 | 4), "{}", r.mnemonic);
                assert_eq!(got, ConformanceLevel::One, "{}", r.mnemonic);
            }
            LevelSource::Exception => {
                assert!(!f.contains_key(r.mnemonic), "{} has a field", r.mnemonic);
                assert_eq!(got, ConformanceLevel::One, "{}", r.mnemonic);
            }
            LevelSource::Manual(t) => {
                assert!(!f.contains_key(r.mnemonic), "{} has a field", r.mnemonic);
                // The VT510's and VT520's at Level 4; the others as the
                // VT420's Table 4-1 names them or not.
                let want = match (t, r.mnemonic) {
                    (DecTerminal::Vt510 | DecTerminal::Vt520, _) => ConformanceLevel::Four,
                    (_, "DSR (keyboard)") => ConformanceLevel::Two,
                    _ => ConformanceLevel::One,
                };
                assert_eq!(got, want, "{}", r.mnemonic);
            }
        }
    }
    assert!(wrong.is_empty(), "\n  {}", wrong.join("\n  "));
    let exceptions: BTreeSet<_> = LEVELS
        .iter()
        .filter(|r| r.source == LevelSource::Exception)
        .map(|r| r.mnemonic)
        .collect();
    assert_eq!(
        exceptions,
        BTreeSet::from(["DECALN", "DECANM", "DECAWM", "RIS"]),
        "Appendix D's RIS, DECALN and DECAWM, and 070-0A's VT52 entry"
    );
}

/// **Every field belongs to a row or is one baud does not dispatch**, and
/// none is both: a control baud gains without a row fails here.
#[test]
fn every_field_is_a_row_or_named_undispatched() {
    let f = fields();
    let rows: BTreeSet<&str> = LEVELS.iter().map(|r| r.mnemonic).collect();
    let not: BTreeSet<&str> = NOT_DISPATCHED.iter().map(|n| n.0).collect();
    let both: Vec<_> = rows.intersection(&not).collect();
    assert!(both.is_empty(), "rows listed undispatched: {both:?}");
    let orphans: Vec<_> = f
        .keys()
        .filter(|m| !rows.contains(m.as_str()) && !not.contains(m.as_str()))
        .collect();
    assert!(orphans.is_empty(), "fields with no row: {orphans:?}");
    let stale: Vec<_> = not.iter().filter(|m| !f.contains_key(**m)).collect();
    assert!(stale.is_empty(), "named undispatched, no field: {stale:?}");
}

fn param(n: u16) -> Option<Param> {
    let mut p = Param::ZERO;
    for d in n.to_string().bytes() {
        p = p.push(StreamByte::from_stream(d).as_ascii_digit().unwrap());
    }
    Some(p)
}

fn csi(marker: Marker, inter: Option<u8>, ps: &[u16], f: u8) -> Csi {
    let ps: Vec<_> = ps.iter().map(|p| param(*p)).collect();
    Csi::parse(
        marker,
        inter.map(StreamByte::from_stream),
        Params::new(&ps),
        FinalByte::from_stream(StreamByte::from_stream(f)).unwrap(),
    )
}

/// The rows the forms parsed here do not reach: those the core dispatches
/// by their own sequences (`ESC #`, `ESC SP`, SCS, the DCS strings), the C0
/// controls every level has, HVP, which parses as CUP and shares its
/// level, and DECSLRM, which `Csi::parse_with` reaches only under DECLRMM.
const CORE_DISPATCHED: &[&str] = &[
    "CR", "LF", "VT", "FF", "BS", "HT", "BEL", "SUB", "LS0", "LS1", "HVP", "S7C1T", "S8C1T", "ACS",
    "DECSWL", "DECDWL", "DECDHL", "DECALN", "SCS", "DECRQSS", "DECRSPS", "DECRSTS", "DECLKD",
    "ReGIS", "Sixel", "DECSLRM", "DECAUPSS",
];

/// **Each row is reached by the forms that perform it**: a control sequence,
/// an `ESC Fe`, a mode or a status report names the row's function.
#[test]
fn the_parsed_forms_name_their_rows() {
    use Marker::{Private as Q, Standard as S};
    let sequences: &[(Csi, &str)] = &[
        (csi(S, None, &[], b'A'), "CUU"),
        (csi(S, None, &[], b'B'), "CUD"),
        (csi(S, None, &[], b'C'), "CUF"),
        (csi(S, None, &[], b'D'), "CUB"),
        (csi(S, None, &[], b'H'), "CUP"),
        (csi(S, None, &[], b'J'), "ED"),
        (csi(S, None, &[], b'K'), "EL"),
        (csi(S, None, &[], b'L'), "IL"),
        (csi(S, None, &[], b'M'), "DL"),
        (csi(S, None, &[], b'P'), "DCH"),
        (csi(S, None, &[], b'X'), "ECH"),
        (csi(S, None, &[], b'@'), "ICH"),
        (csi(S, None, &[], b'r'), "DECSTBM"),
        (csi(S, Some(b'"'), &[1], b'q'), "DECSCA"),
        (csi(Q, None, &[], b'J'), "DECSED"),
        (csi(Q, None, &[], b'K'), "DECSEL"),
        (csi(S, None, &[], b'c'), "DA"),
        (csi(S, None, &[5], b'n'), "DSR"),
        (csi(S, None, &[6], b'n'), "CPR"),
        (csi(S, None, &[0], b'x'), "DECREQTPARM"),
        (csi(S, None, &[1], b'q'), "DECLL"),
        (csi(S, Some(b'&'), &[], b'u'), "DECRQUPSS"),
        (csi(S, Some(b'"'), &[], b'v'), "DECRQDE"),
        (csi(S, None, &[2], b'U'), "NP"),
        (csi(S, None, &[], b'V'), "PP"),
        (csi(S, Some(b' '), &[3], b'P'), "PPA"),
        (csi(S, Some(b' '), &[], b'Q'), "PPR"),
        (csi(S, Some(b' '), &[1], b'R'), "PPB"),
        (csi(S, Some(b'\''), &[], b'z'), "DECELR"),
        (csi(S, Some(b'\''), &[], b'{'), "DECSLE"),
        (csi(S, Some(b'\''), &[], b'w'), "DECEFR"),
        (csi(S, Some(b'\''), &[], b'|'), "DECRQLP"),
        (csi(S, Some(b'!'), &[], b'p'), "DECSTR"),
        (csi(S, Some(b'"'), &[61], b'p'), "DECSCL"),
        (csi(S, None, &[], b'g'), "TBC"),
        (csi(Q, None, &[5], b'W'), "DECST8C"),
        (csi(S, None, &[], b'S'), "SU"),
        (csi(S, None, &[], b'T'), "SD"),
        (csi(S, None, &[1], b'm'), "SGR"),
        (csi(S, Some(b' '), &[1], b'q'), "DECSCUSR"),
        (csi(S, Some(b' '), &[1], b't'), "DECSWBV"),
        (csi(S, Some(b' '), &[1], b'u'), "DECSMBV"),
        (csi(S, None, &[0], b'i'), "MC"),
        (csi(S, Some(b'\''), &[], b'}'), "DECIC"),
        (csi(S, Some(b'\''), &[], b'~'), "DECDC"),
        (csi(S, Some(b'$'), &[80], b'|'), "DECSCPP"),
        (csi(S, Some(b'$'), &[0], b'~'), "DECSSDT"),
        (csi(S, Some(b'$'), &[0], b'}'), "DECSASD"),
        (csi(S, Some(b'$'), &[2], b'p'), "DECRQM"),
        (csi(Q, Some(b'$'), &[2], b'p'), "DECRQM"),
        (csi(S, None, &[24], b't'), "DECSLPP"),
        (csi(S, Some(b'$'), &[], b'v'), "DECCRA"),
        (csi(S, Some(b'$'), &[], b'x'), "DECFRA"),
        (csi(S, Some(b'$'), &[], b'z'), "DECERA"),
        (csi(S, Some(b'$'), &[], b'{'), "DECSERA"),
        (csi(S, Some(b'$'), &[], b'r'), "DECCARA"),
        (csi(S, Some(b'$'), &[], b't'), "DECRARA"),
        (csi(S, Some(b'*'), &[], b'x'), "DECSACE"),
        (csi(S, Some(b'*'), &[], b'y'), "DECRQCRA"),
        (csi(S, Some(b'$'), &[1], b'w'), "DECRQPSR"),
        (csi(S, Some(b'$'), &[1], b'u'), "DECRQTSR"),
    ];
    let escapes: &[(u8, &str)] = &[
        (b'7', "DECSC"),
        (b'8', "DECRC"),
        (b'M', "RI"),
        (b'D', "IND"),
        (b'E', "NEL"),
        (b'c', "RIS"),
        (b'=', "DECKPAM"),
        (b'>', "DECKPNM"),
        (b'H', "HTS"),
        (b'Z', "DECID"),
        (b'6', "DECBI"),
        (b'9', "DECFI"),
        (b'N', "SS2"),
        (b'O', "SS3"),
        (b'n', "LS2"),
        (b'o', "LS3"),
        (b'~', "LS1R"),
        (b'}', "LS2R"),
        (b'|', "LS3R"),
    ];
    let ansi: &[(u16, &str)] = &[(2, "KAM"), (4, "IRM"), (12, "SRM"), (20, "LNM")];
    let private: &[(u16, &str)] = &[
        (1, "DECCKM"),
        (2, "DECANM"),
        (3, "DECCOLM"),
        (4, "DECSCLM"),
        (5, "DECSCNM"),
        (6, "DECOM"),
        (7, "DECAWM"),
        (8, "DECARM"),
        (18, "DECPFF"),
        (19, "DECPEX"),
        (25, "DECTCEM"),
        (42, "DECNRCM"),
        (43, "DECGEPM"),
        (66, "DECNKM"),
        (67, "DECBKM"),
        (69, "DECLRMM"),
        (80, "DECSDM"),
        (34, "DECRLM"),
        (35, "DECHEBM"),
        (36, "DECHEM"),
        (95, "DECNCSM"),
        (96, "DECRLCM"),
        (117, "DECECM"),
    ];
    let reports: &[(DecStatus, &str)] = &[
        (DecStatus::ExtendedCursor, "DECXCPR"),
        (DecStatus::Printer, "DSR (printer)"),
        (DecStatus::UserKeys, "DSR (UDK)"),
        (DecStatus::Keyboard, "DSR (keyboard)"),
        (DecStatus::LocatorStatus, "DSR (locator)"),
        (DecStatus::MacroSpace, "DECMSR"),
        (
            DecStatus::MemoryChecksum { id: Param::ZERO },
            "DSR (checksum)",
        ),
        (DecStatus::DataIntegrity, "DSR (data integrity)"),
        (DecStatus::Sessions, "DSR (sessions)"),
    ];
    let mut reached = BTreeSet::new();
    let mut wrong = Vec::new();
    let mut check = |what: String, got: Option<Function>, want: &'static str| match got
        .map(|f| f.row().mnemonic)
    {
        Some(m) if m == want => {
            reached.insert(want);
        }
        other => wrong.push(format!("{what} names {other:?}, not {want}")),
    };
    for (i, (c, want)) in sequences.iter().enumerate() {
        check(format!("sequence {i}"), c.function(), want);
    }
    for (b, want) in escapes {
        check(
            format!("ESC {}", *b as char),
            Esc::parse(StreamByte::from_stream(*b)).function(),
            want,
        );
    }
    for (n, want) in ansi {
        check(
            format!("mode {n}"),
            AnsiMode::from_wire(param(*n)).function(),
            want,
        );
    }
    for (n, want) in private {
        check(
            format!("mode ?{n}"),
            PrivateMode::from_wire(param(*n)).function(),
            want,
        );
    }
    for (k, want) in reports {
        check(format!("{k:?}"), Some(k.function()), want);
    }
    assert!(wrong.is_empty(), "\n  {}", wrong.join("\n  "));
    let rows: BTreeSet<&str> = LEVELS.iter().map(|r| r.mnemonic).collect();
    let core: BTreeSet<&str> = CORE_DISPATCHED.iter().copied().collect();
    let unreached: Vec<_> = rows
        .iter()
        .filter(|m| !reached.contains(*m) && !core.contains(*m))
        .collect();
    assert!(unreached.is_empty(), "rows no form reaches: {unreached:?}");
}

/// **An extension acts at every level**: xterm's and ECMA-48's own
/// controls name no function, so no level refuses them.
#[test]
fn a_control_no_dec_level_defines_acts_at_every_level() {
    use Marker::{Private as Q, Standard as S};
    for (i, c) in [
        csi(S, None, &[], b'G'),
        csi(S, None, &[], b'd'),
        csi(S, None, &[], b'b'),
        csi(S, None, &[], b'E'),
        csi(S, None, &[], b'I'),
        csi(S, None, &[], b'Z'),
        csi(S, None, &[], b's'),
        csi(S, None, &[18], b't'),
        csi(Q, None, &[1049], b'h'),
        csi(Marker::Gt, None, &[], b'q'),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(introduced_by(c.function()), Introduced::Extension, "{i}");
        for at in [
            ConformanceLevel::One,
            ConformanceLevel::Two,
            ConformanceLevel::Three,
            ConformanceLevel::Four,
        ] {
            assert!(introduced_by(c.function()).admits(at));
        }
    }
}

/// **DECID is the one function a note removes**: Levels 1 and 2 only.
#[test]
fn decid_acts_at_levels_one_and_two_only() {
    let i = Function::Decid.introduced();
    assert!(i.admits(ConformanceLevel::One) && i.admits(ConformanceLevel::Two));
    assert!(!i.admits(ConformanceLevel::Three) && !i.admits(ConformanceLevel::Four));
}

/// **The character sets' levels**: ASCII, line drawing, UK and the NRC
/// sets at Level 1, DEC Supplemental and ISO Latin-1 from Level 2, the
/// VT500's sets at Level 4; and Level 1 designates into G0 and G1 alone.
#[test]
fn each_character_set_is_its_levels() {
    use ConformanceLevel::*;
    for (set, least) in [
        (Charset::Ascii, One),
        (Charset::Uk, One),
        (Charset::DecGraphics, One),
        (Charset::Dec(DecSet::JisRoman), One),
        (Charset::National(Nrcs::German), One),
        (Charset::National(Nrcs::Portuguese), One),
        (Charset::Dec(DecSet::Supplemental), Two),
        (Charset::Dec(DecSet::SupplementalGraphics), Two),
        (Charset::Dec(DecSet::Technical), Two),
        (Charset::Iso96(Iso96::Latin1), Two),
        (Charset::National(Nrcs::Hebrew), Four),
        (Charset::Dec(DecSet::Cyrillic), Four),
        (Charset::Iso96(Iso96::Latin2), Four),
    ] {
        assert_eq!(set.introduced().least(), least, "{set:?}");
    }
    assert!(GSlot::G1.designable(One) && !GSlot::G2.designable(One) && !GSlot::G3.designable(One));
    assert!(GSlot::G3.designable(Two));
}

/// **SGR at Level 1**: 0, 1, 4, 5, 7 and 8; colour and the "off" forms
/// from Level 2; the rest no DEC level's.
#[test]
fn each_sgr_parameter_is_its_levels() {
    use ConformanceLevel::*;
    for v in [0, 1, 4, 5, 7, 8] {
        assert!(sgr_introduced(v).admits(One), "{v}");
    }
    for v in [
        22, 24, 25, 27, 28, 30, 37, 38, 39, 40, 48, 49, 58, 59, 90, 107,
    ] {
        assert!(
            !sgr_introduced(v).admits(One) && sgr_introduced(v).admits(Two),
            "{v}"
        );
    }
    for v in [2, 3, 9, 21, 23, 29, 53] {
        assert_eq!(sgr_introduced(v), Introduced::Extension, "{v}");
    }
}

/// **Level 4 executes every function but DECID**, so `csi_final` asks the
/// table only below Level 4: no control sequence's row is removed above a
/// level, and DECID, the one that is, is an `ESC Fe`.
#[test]
fn level_four_executes_every_control_sequence_function() {
    let removed: Vec<_> = LEVELS
        .iter()
        .filter(|r| !r.introduced.admits(ConformanceLevel::Four))
        .map(|r| r.mnemonic)
        .collect();
    assert_eq!(removed, ["DECID"]);
    assert_eq!(
        Esc::parse(StreamByte::from_stream(b'Z')).function(),
        Some(Function::Decid)
    );
}
