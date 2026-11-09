//! **Built without `read-record`, the record vouches for nothing**: every
//! part a read made is `Renumbered::Unrecorded` and `ReadEncoding::Other`,
//! with nothing off its path and no move, so an embedder that files lines
//! by the record files none of them as the read's own. Runs only in the
//! build without the defaults (`scripts/test-features.sh`); every other
//! build of the suite has the record on.
#![cfg(not(feature = "read-record"))]
mod support;

use baud_core::*;
use baud_domain::{BytePos, ReadLen, WinSize};

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 10,
};

/// One read, fed whole, and its record. Break: return the recording's own
/// record from `take_record` in the build without the feature.
#[test]
fn without_the_feature_every_record_vouches_for_nothing() {
    support::with_term(&WS, |t| {
        for bytes in [
            &b"plain text\r\n"[..],
            b"\x1b#8\x1b[5;1H\x1b[1;1;2;5;1$r",
            b"\x1b[2;6r\x1b[3S",
            b"\x1b]7;file:///tmp\x07\x1b]133;A\x07",
        ] {
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let rec = t.take_record();
            assert_eq!(rec.renumbered(), Some(Renumbered::Unrecorded), "{bytes:?}");
            assert_eq!(rec.encoding(), ReadEncoding::Other, "{bytes:?}");
            assert_eq!(rec.off_path(Screen::Normal), None, "{bytes:?}");
            assert_eq!(rec.moves().iter().count(), 0, "{bytes:?}");
            assert_eq!(rec.folders().iter().count(), 0, "{bytes:?}");
        }
    });
}
