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
        px: None,
    }
}

/// `e` with every modifier held: the widest `Cb` its button can carry.
fn all_mods(e: MouseEvent) -> MouseEvent {
    MouseEvent {
        mods: MouseMods::SHIFT
            .with(MouseMods::META)
            .with(MouseMods::CONTROL),
        ..e
    }
}

fn enc(r: Report) -> Vec<u8> {
    r.encode(C1Transmit::SevenBit).as_bytes().to_vec()
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

/// A `Param` pushed from ASCII digits, as [`one`] builds a `OneBased`.
fn param(n: u16) -> Param {
    let mut p = Param::ZERO;
    for b in n.to_string().bytes() {
        p = p.push(StreamByte::from_stream(b).as_ascii_digit().unwrap());
    }
    p
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
        enc(Report::PrimaryDeviceAttributes(
            DeviceAttribute::Vt102,
            SixelClaim::No
        )),
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
        Report::PrimaryDeviceAttributes(DeviceAttribute::Vt102, SixelClaim::No),
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
        Report::PrimaryDeviceAttributes(DeviceAttribute::Vt102, SixelClaim::No),
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

/// **The X10 family writes a release as code 3, and SGR keeps the button.**
/// `[[source: ctlseqs]]`: "3=release" for the X10 scheme; for SGR, "A
/// different final character is used for button release to resolve the X10
/// ambiguity regarding which button was released."
#[test]
fn x10_family_releases_are_code_three_and_sgr_keeps_the_button() {
    let release = mouse(MouseButton::Right, MouseKind::Release, 0, 0);
    assert_eq!(enc(Report::MouseX10(release)), b"\x1b[M\x23\x21\x21");
    assert_eq!(enc(Report::MouseUtf8(release)), b"\x1b[M\x23\x21\x21");
    assert_eq!(enc(Report::MouseUrxvt(release)), b"\x1b[35;1;1M");
    assert_eq!(enc(Report::MouseSgr(release)), b"\x1b[<2;1;1m");
}

/// `?1005`'s bytes: X10 below 128, then UTF-8 -- `[[source: ctlseqs]]`'s
/// "expanding the maximum encodable position from 223 to 2015", whose last
/// position is `2015 + 32` = U+07FF.
#[test]
fn utf8_mouse_reports_are_x10_until_127_and_utf8_after() {
    let at = |col, row| {
        enc(Report::MouseUtf8(mouse(
            MouseButton::Left,
            MouseKind::Press,
            col,
            row,
        )))
    };
    assert_eq!(at(94, 0), b"\x1b[M\x20\x7f\x21");
    assert_eq!(at(95, 0), b"\x1b[M\x20\xc2\x80\x21");
    assert_eq!(at(0, 2014), b"\x1b[M\x20\x21\xdf\xbf");
    // In 8-bit controls the introducer is one byte and the rest unchanged.
    assert_eq!(
        Report::MouseUtf8(mouse(MouseButton::Left, MouseKind::Press, 95, 0))
            .encode(C1Transmit::EightBit)
            .as_bytes(),
        b"\x9bM\x20\xc2\x80\x21"
    );
}

/// `?1015`: "CSI followed by semicolon-separated encoded button value, the
/// Px and Py ordinates and final character M", the button value X10's.
#[test]
fn urxvt_mouse_reports_have_the_documented_shape() {
    assert_eq!(
        enc(Report::MouseUrxvt(mouse(
            MouseButton::Left,
            MouseKind::Press,
            399,
            41
        ))),
        b"\x1b[32;400;42M"
    );
    assert_eq!(
        enc(Report::MouseUrxvt(mouse(
            MouseButton::WheelUp,
            MouseKind::Press,
            0,
            0
        ))),
        b"\x1b[96;1;1M"
    );
}

/// `?1016`: "the same mouse response format as the 1006 control, but report
/// position in pixels", the pixel one-based on the wire.
#[test]
fn sgr_pixel_mouse_reports_carry_the_pixel_one_based() {
    let px = PointerPx {
        x: ImagePx::of(799),
        y: ImagePx::ZERO,
    };
    assert_eq!(
        enc(Report::MouseSgrPixels(
            mouse(MouseButton::Middle, MouseKind::Press, 39, 0),
            px
        )),
        b"\x1b[<1;800;1M"
    );
    assert_eq!(
        enc(Report::MouseSgrPixels(
            mouse(MouseButton::Middle, MouseKind::Release, 39, 0),
            px
        )),
        b"\x1b[<1;800;1m"
    );
}

/// **The encoding a DECSET or DECRST leaves**: a set selects, a reset
/// undoes only the one in force -- xterm's "a reset is only effective
/// against the matching mode".
#[test]
fn a_set_selects_an_encoding_and_a_reset_undoes_only_its_own() {
    use MouseEncoding::*;
    for from in [X10, Utf8, Sgr, Urxvt, SgrPixels] {
        for mode in [Utf8, Sgr, Urxvt, SgrPixels] {
            assert_eq!(
                from.after(mode, ModeChange::Set),
                mode,
                "{from:?} then {mode:?} h"
            );
            let want = if from == mode { X10 } else { from };
            assert_eq!(
                from.after(mode, ModeChange::Reset),
                want,
                "{from:?} then {mode:?} l"
            );
        }
    }
    assert_eq!(X10.max_ordinate(), Some(223));
    assert_eq!(Utf8.max_ordinate(), Some(2015));
    assert_eq!(Sgr.max_ordinate(), None);
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

    // **The widest is the palette reply, and this test could not see it.**
    // `ReportBytes::push` drops silently past `MAX_LEN`, and for as long as
    // this test named only two variants a new one could be added, be truncated
    // by two bytes, and leave the assertion green -- which is exactly what
    // happened when `Report::Color` landed at 22 against a `MAX_LEN` of 21.
    // The tell is the terminator: a truncated reply loses its last byte, and a
    // reader waiting for `ST` waits forever.
    let color = enc(Report::Color {
        index: Chan::from_u8(255),
        r: 0xff,
        g: 0xff,
        b: 0xff,
        term: StringTerm::St,
    });
    assert_eq!(color, b"\x1b]4;255;rgb:ffff/ffff/ffff\x1b\\");

    // **And then the `?2048` report took the title** when its pixel fields
    // stopped being zero: four five-digit fields where there had been two
    // and two zeros. Every field at `u16::MAX` is the widest it can encode.
    let inband = enc(Report::InbandResize {
        rows: Rows::of_count(65535),
        cols: Cols::of_index(65535),
        height: ImagePx::of(u16::MAX),
        width: ImagePx::of(u16::MAX),
    });
    assert_eq!(inband, b"\x1b[48;65535;65535;65535;65535t");

    // **And then `?1001`'s long release report under SGR**: six positions
    // where the `?2048` report has four fields, every one at the widest a
    // `Col` or `Row` can write.
    let far = WireCell::of(CellAt::new(Col::at_index(65534), Row::from_index(65534)));
    let highlight = enc(Report::Highlight(
        HighlightEnd::Moved {
            start: far,
            end: far,
            mouse: MouseAt::Cell(far),
        },
        MouseEncoding::Sgr,
    ));
    assert_eq!(highlight, b"\x1b[<65535;65535;65535;65535;65535;65535T");
    assert_eq!(
        highlight.len(),
        Report::MAX_LEN,
        "the widest report is this one"
    );

    let x10_far = WireCell::of(CellAt::new(Col::at_index(222), Row::from_index(222)));
    let utf8_far = WireCell::of(CellAt::new(Col::at_index(2014), Row::from_index(2014)));
    // Every variant, at its widest, ending in the byte that terminates it.
    // A variant absent from this list is a variant nothing bounds.
    for (name, r, last) in [
        (
            "cpr",
            Report::CursorPosition {
                row: one(65535),
                col: one(65535),
            },
            b'R',
        ),
        ("dsr", Report::OperatingStatus, b'n'),
        (
            "decxcpr",
            Report::ExtendedCursorPosition {
                row: one(65535),
                col: one(65535),
            },
            b'R',
        ),
        (
            "dec status, keyboard",
            Report::DecStatus(DecReply::KeyboardUnknown),
            b'n',
        ),
        ("decmsr", Report::MacroSpace, b'{'),
        ("decreptparm", Report::TerminalParameters { sol: 3 }, b'x'),
        ("decaupss", Report::PreferredSupplemental, b'\\'),
        (
            "decrpde",
            Report::DisplayedExtent {
                rows: Rows::of_count(65535),
                cols: Cols::of_index(65535),
            },
            b'w',
        ),
        (
            "key option",
            Report::KeyOption {
                which: KeyOption::Format,
                value: 255,
            },
            b'f',
        ),
        (
            "colour scheme",
            Report::ColorScheme(ColorScheme::Light),
            b'n',
        ),
        (
            "da",
            Report::PrimaryDeviceAttributes(DeviceAttribute::Vt102, SixelClaim::No),
            b'c',
        ),
        (
            "sgr mouse",
            Report::MouseSgr(mouse(MouseButton::WheelDown, MouseKind::Motion, 2047, 639)),
            b'M',
        ),
        ("vt52 id", Report::Vt52Identity, b'Z'),
        (
            "graphics geometry",
            Report::Graphics {
                item: param(65535),
                reply: GraphicsReply::Geometry {
                    width: ImagePx::of(u16::MAX),
                    height: ImagePx::of(u16::MAX),
                },
            },
            b'S',
        ),
        (
            "text area pixels",
            Report::TextAreaPixels {
                height: ImagePx::of(u16::MAX),
                width: ImagePx::of(u16::MAX),
            },
            b't',
        ),
        (
            "cell pixels",
            Report::CellPixels {
                height: ImagePx::of(u16::MAX),
                width: ImagePx::of(u16::MAX),
            },
            b't',
        ),
        (
            "text area chars",
            Report::TextAreaChars {
                rows: Rows::of_count(65535),
                cols: Cols::of_index(65535),
            },
            b't',
        ),
        ("version", Report::Version, b'\\'),
        (
            "area checksum",
            Report::AreaChecksum {
                id: param(65535),
                sum: Checksum::new(),
            },
            b'\\',
        ),
        (
            "colour, BEL",
            Report::Color {
                index: Chan::from_u8(255),
                r: 0xff,
                g: 0xff,
                b: 0xff,
                term: StringTerm::Bel,
            },
            0x07,
        ),
        // The six that were missing from this list while it said a variant
        // absent from it is a variant nothing bounds.
        ("focus", Report::Focus(Focus::Gained), b'I'),
        ("kitty flags", Report::KittyFlags(KittyFlags::ALL), b'u'),
        (
            "inband resize",
            Report::InbandResize {
                rows: Rows::of_count(65535),
                cols: Cols::of_index(65535),
                height: ImagePx::of(u16::MAX),
                width: ImagePx::of(u16::MAX),
            },
            b't',
        ),
        (
            "dynamic colour",
            Report::DynamicColor {
                which: DynamicColor::Background,
                r: 0xff,
                g: 0xff,
                b: 0xff,
                term: StringTerm::St,
            },
            b'\\',
        ),
        (
            "mode status",
            Report::ModeStatus {
                marker: Marker::Private,
                mode: Param::ZERO,
                state: ModeState::PermanentlyReset,
            },
            b'y',
        ),
        (
            "x10 mouse",
            Report::MouseX10(mouse(MouseButton::Left, MouseKind::Press, 222, 222)),
            b' ' + 223,
        ),
        // The widest of each encoding `?1005`, `?1015` and `?1016` brought:
        // every modifier on the widest button code the type can build, at
        // the last position each can carry.
        (
            "utf-8 mouse",
            Report::MouseUtf8(all_mods(mouse(
                MouseButton::WheelDown,
                MouseKind::Motion,
                2014,
                2014,
            ))),
            0xbf,
        ),
        (
            "urxvt mouse",
            Report::MouseUrxvt(all_mods(mouse(
                MouseButton::WheelDown,
                MouseKind::Motion,
                2047,
                639,
            ))),
            b'M',
        ),
        (
            "sgr-pixels mouse",
            Report::MouseSgrPixels(
                all_mods(mouse(MouseButton::WheelDown, MouseKind::Release, 2047, 639)),
                PointerPx {
                    x: ImagePx::of(u16::MAX - 1),
                    y: ImagePx::of(u16::MAX - 1),
                },
            ),
            b'm',
        ),
        // `?1001`'s release reports, each at the widest its encoding names.
        (
            "highlight, sgr, long",
            Report::Highlight(
                HighlightEnd::Moved {
                    start: far,
                    end: far,
                    mouse: MouseAt::Cell(far),
                },
                MouseEncoding::Sgr,
            ),
            b'T',
        ),
        (
            "highlight, sgr, short",
            Report::Highlight(HighlightEnd::Same(far), MouseEncoding::Sgr),
            b't',
        ),
        (
            "highlight, urxvt, long",
            Report::Highlight(
                HighlightEnd::Moved {
                    start: far,
                    end: far,
                    mouse: MouseAt::Cell(far),
                },
                MouseEncoding::Urxvt,
            ),
            b'T',
        ),
        // Under `?1016` the mouse is a pixel, the widest one a press can
        // carry: `u16::MAX - 1`, written `+ 1`, five digits like a cell.
        (
            "highlight, sgr-pixels, long",
            Report::Highlight(
                HighlightEnd::Moved {
                    start: far,
                    end: far,
                    mouse: MouseAt::Px(PointerPx {
                        x: ImagePx::of(u16::MAX - 1),
                        y: ImagePx::of(u16::MAX - 1),
                    }),
                },
                MouseEncoding::SgrPixels,
            ),
            b'T',
        ),
        (
            "highlight, x10, long",
            Report::Highlight(
                HighlightEnd::Moved {
                    start: x10_far,
                    end: x10_far,
                    mouse: MouseAt::Cell(x10_far),
                },
                MouseEncoding::X10,
            ),
            b' ' + 223,
        ),
        (
            "highlight, utf-8, long",
            Report::Highlight(
                HighlightEnd::Moved {
                    start: utf8_far,
                    end: utf8_far,
                    mouse: MouseAt::Cell(utf8_far),
                },
                MouseEncoding::Utf8,
            ),
            0xbf,
        ),
    ] {
        // **Exhaustive, so this list cannot fall behind the enum again.** A
        // new `Report` variant stops this match compiling, and the edit it
        // forces is here, beside the list it has to join.
        match r {
            Report::Focus(_)
            | Report::CursorPosition { .. }
            | Report::OperatingStatus
            | Report::ColorScheme(_)
            | Report::PrimaryDeviceAttributes(..)
            | Report::Vt52Identity
            | Report::KittyFlags(_)
            | Report::InbandResize { .. }
            | Report::TextAreaPixels { .. }
            | Report::CellPixels { .. }
            | Report::TextAreaChars { .. }
            | Report::Version
            | Report::AreaChecksum { .. }
            | Report::Color { .. }
            | Report::DynamicColor { .. }
            | Report::ModeStatus { .. }
            | Report::MouseSgr(_)
            | Report::MouseX10(_)
            | Report::MouseUtf8(_)
            | Report::MouseUrxvt(_)
            | Report::MouseSgrPixels(..)
            | Report::Highlight(..)
            | Report::Graphics { .. }
            | Report::ExtendedCursorPosition { .. }
            | Report::DecStatus(_)
            | Report::MacroSpace
            | Report::TerminalParameters { .. }
            | Report::PreferredSupplemental
            | Report::DisplayedExtent { .. }
            | Report::KeyOption { .. } => {}
        }
        let b = enc(r);
        assert!(
            b.len() <= Report::MAX_LEN,
            "{name}: {} bytes exceeds MAX_LEN {}",
            b.len(),
            Report::MAX_LEN
        );
        assert_eq!(*b.last().unwrap(), last, "{name}: the final byte survived");
    }
}

/// **Under `?1016` the long highlight report's mouse is a pixel and the
/// region is cells**: `[[source: ctlseqs]]` calls startx, starty, endx and
/// endy "character positions" and mousex, mousey "the location of the mouse
/// at button up", which SGR-Pixels reports "in pixels rather than character
/// cells". `HighlightEnd::of` picks the unit from the encoding, refuses a
/// long form whose pixel is missing or at `u16::MAX`, and writes the short
/// form, which has no mouse field, without one.
#[test]
fn a_highlight_report_s_mouse_is_a_pixel_under_sgr_pixels_alone() {
    let cell = |c, r| CellAt::new(Col::at_index(c), Row::from_index(r));
    let px = |x, y| {
        Some(PointerPx {
            x: ImagePx::of(x),
            y: ImagePx::of(y),
        })
    };
    let (start, end, mouse) = (cell(4, 2), cell(9, 4), cell(9, 4));
    let of = |px, encoding| HighlightEnd::of(start, end, mouse, px, encoding);
    let pixels = of(px(95, 50), MouseEncoding::SgrPixels).unwrap();
    assert_eq!(
        enc(Report::Highlight(pixels, MouseEncoding::SgrPixels)),
        b"\x1b[<5;3;10;5;96;51T"
    );
    // Under 1006 the same release writes the mouse's cell, pixel or not.
    let cells = of(px(95, 50), MouseEncoding::Sgr).unwrap();
    assert_eq!(
        enc(Report::Highlight(cells, MouseEncoding::Sgr)),
        b"\x1b[<5;3;10;5;10;5T"
    );
    for missing in [None, px(u16::MAX, 50), px(95, u16::MAX)] {
        assert_eq!(of(missing, MouseEncoding::SgrPixels), None, "{missing:?}");
    }
    let short = HighlightEnd::of(start, start, mouse, None, MouseEncoding::SgrPixels).unwrap();
    assert_eq!(
        enc(Report::Highlight(short, MouseEncoding::SgrPixels)),
        b"\x1b[<5;3t"
    );
    // The widest pixel a press can carry is five digits on the wire, as
    // the widest cell is, so `MAX_LEN` holds.
    let far = WireCell::of(cell(65534, 65534));
    let widest = enc(Report::Highlight(
        HighlightEnd::Moved {
            start: far,
            end: far,
            mouse: MouseAt::Px(PointerPx {
                x: ImagePx::of(u16::MAX - 1),
                y: ImagePx::of(u16::MAX - 1),
            }),
        },
        MouseEncoding::SgrPixels,
    ));
    assert_eq!(widest, b"\x1b[<65535;65535;65535;65535;65535;65535T");
    assert_eq!(widest.len(), Report::MAX_LEN);
}
