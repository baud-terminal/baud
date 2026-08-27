//! Every report baud can send, encoded and checked against the specification.
//!
//! These live in `baud-domain` rather than in `baud-core` because the encoding
//! is a property of the protocol, not of a terminal: a report has one correct
//! shape whatever produced it. Driving a whole `Term` to check a byte string
//! would test the dispatch as well, which `baud-core/tests/vt_replies.rs`
//! already does.
//!
//! Shapes are from `[[source: ctlseqs]]` § "Functions using CSI", quoted at
//! each case rather than recalled.

use baud_domain::*;

/// A mouse event at 1-based `(col, row)`, built the way the core does.
fn mouse(button: MouseButton, kind: MouseKind, col: usize, row: usize) -> MouseEvent {
    MouseEvent {
        button,
        mods: MouseMods::NONE,
        kind,
        col: Col::at_index(col),
        row: Row::from_index(row),
    }
}

fn enc(r: Report) -> Vec<u8> {
    r.encode().as_bytes().to_vec()
}

/// Build a `OneBased` the way the parser does -- by pushing ASCII digits --
/// so the tests cannot construct a value the wire could not carry.
fn one(n: u16) -> OneBased {
    let mut p = Param::ZERO;
    for b in n.to_string().bytes() {
        p = p.push(StreamByte::from_stream(b).as_ascii_digit().unwrap());
    }
    OneBased::from_param(Some(p))
}

/// `[[source: ctlseqs]]`: "Report Cursor Position (CPR) [row;column]. Result is
/// CSI r ; c R".
#[test]
fn cpr_has_the_shape_ctlseqs_specifies() {
    assert_eq!(
        enc(Report::CursorPosition {
            row: one(1),
            col: one(1)
        }),
        b"\x1b[1;1R"
    );
    assert_eq!(
        enc(Report::CursorPosition {
            row: one(7),
            col: one(13)
        }),
        b"\x1b[7;13R"
    );
}

/// Multi-digit coordinates are not truncated or reversed. The grid goes to
/// 2048 columns, so a three-digit column is ordinary, not an edge case.
#[test]
fn cpr_encodes_wide_coordinates() {
    assert_eq!(
        enc(Report::CursorPosition {
            row: one(50),
            col: one(200)
        }),
        b"\x1b[50;200R"
    );
    assert_eq!(
        enc(Report::CursorPosition {
            row: one(999),
            col: one(2048)
        }),
        b"\x1b[999;2048R"
    );
}

/// `[[source: ctlseqs]]`: "Status Report. Result ("OK") is CSI 0 n".
#[test]
fn dsr_operating_status_is_csi_zero_n() {
    assert_eq!(enc(Report::OperatingStatus), b"\x1b[0n");
}

/// The Primary DA response carries the `?` private marker and the identity
/// number. baud is a VT102.
#[test]
fn primary_da_answers_as_a_vt102() {
    assert_eq!(
        enc(Report::PrimaryDeviceAttributes(DeviceAttribute::Vt102)),
        b"\x1b[?6c"
    );
}

/// No two reports share an encoding.
///
/// This is the "never answer a query with the wrong shape" rule as a test: an
/// application distinguishes reports by their final byte and marker, so two
/// variants that encoded alike would be indistinguishable to it.
#[test]
fn reports_are_mutually_distinguishable() {
    let all = [
        Report::CursorPosition {
            row: one(1),
            col: one(1),
        },
        Report::OperatingStatus,
        Report::PrimaryDeviceAttributes(DeviceAttribute::Vt102),
    ];
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            if i != j {
                assert_ne!(enc(*a), enc(*b), "variants {i} and {j} encode alike");
            }
        }
    }
}

/// Every report is a well-formed CSI sequence: `ESC [`, then parameter bytes,
/// then exactly one final byte in the range ECMA-48 allows.
#[test]
fn every_report_is_a_well_formed_csi() {
    let all = [
        Report::CursorPosition {
            row: one(1),
            col: one(1),
        },
        Report::CursorPosition {
            row: one(999),
            col: one(2048),
        },
        Report::OperatingStatus,
        Report::PrimaryDeviceAttributes(DeviceAttribute::Vt102),
    ];
    for r in all {
        let b = enc(r);
        assert_eq!(b[0], 0x1b, "{r:?} must start with ESC");
        assert_eq!(b[1], b'[', "{r:?} must use CSI");
        let last = b.len() - 1;
        assert!(
            FinalByte::from_stream(StreamByte::from_stream(b[last])).is_some(),
            "{r:?} must end on a legal final byte"
        );
        for x in &b[2..last] {
            assert!(
                (0x30..0x40).contains(x),
                "{r:?} has a non-parameter byte in its parameter string"
            );
        }
    }
}

/// An SGR mouse report, `CSI < Cb ; Px ; Py M`.
///
/// `[[source: ctlseqs]]`, SGR (1006): "CSI < followed by semicolon-separated
/// encoded button value, Px and Py ordinates and a final character which is M
/// for button press and m for button release."
#[test]
fn sgr_mouse_reports_have_the_documented_shape() {
    assert_eq!(
        enc(Report::MouseSgr(mouse(
            MouseButton::Left,
            MouseKind::Press,
            0,
            0
        ))),
        b"\x1b[<0;1;1M"
    );
    assert_eq!(
        enc(Report::MouseSgr(mouse(
            MouseButton::Right,
            MouseKind::Release,
            399,
            41
        ))),
        b"\x1b[<2;400;42m"
    );
}

/// The buffer is big enough for the longest report there is.
///
/// `Report::MAX_LEN` is a fixed array size and a silent truncation there would
/// produce a report missing its final byte -- which an application would wait
/// for forever.
#[test]
fn report_encodings_fit() {
    // The mouse report is the long one: `ESC [ <` plus three five-digit
    // fields and two separators and a final byte.
    // The widest coordinates the grid can produce: baud-core's MAX_COLS and
    // MAX_ROWS, spelled here because the domain crate does not depend on them.
    let widest = Report::MouseSgr(mouse(MouseButton::WheelDown, MouseKind::Motion, 2047, 639));
    let b = enc(widest);
    assert!(
        b.len() <= Report::MAX_LEN,
        "{} bytes exceeds MAX_LEN {}",
        b.len(),
        Report::MAX_LEN
    );
    assert_eq!(*b.last().unwrap(), b'M', "the final byte survived");
    assert_eq!(b, b"\x1b[<97;2048;640M", "65 (wheel down) + 32 (motion)");
    // And the CPR still fits, which was the previous widest.
    let cpr = enc(Report::CursorPosition {
        row: one(65535),
        col: one(65535),
    });
    assert_eq!(cpr, b"\x1b[65535;65535R");
}
