//! **`kitten icat` ends its detection on baud's primary DA**, by default
//! and under the Sixel claim.
//!
//! kitten's default transfer mode queries first, three `a=q` and then
//! `CSI c`, and waits up to ten seconds for the DA. It ends the wait only on
//! a CSI whose payload passes the check at `kittens/icat/detect.go:93`
//! (`[[source: kitty-icat-detect]]`, kitty v0.49.2; the file at kitty master
//! de20cf35 is the same bytes, sha256 `60a0864d...`):
//!
//! ```go
//! if len(payload) > 3 && payload[0] == '?' && payload[len(payload)-1] == 'c' {
//! ```
//!
//! The payload is the CSI after `ESC [`, its final byte included: the loop
//! writes each byte to the buffer before testing it
//! (`[[source: kitty-escape-code-parser]]`). So VT102's `CSI ? 6 c` is the
//! three bytes `?6c` and fails, and kitten times out with nothing drawn,
//! which is why baud's default answer is the VT100 with Advanced Video
//! Option, `CSI ? 1 ; 2 c` (`[[source: ctlseqs]]`, "VT100 with Advanced
//! Video Option"; the `CSI c` row of `docs/vt-coverage.md`).

mod support;

use baud_core::*;
use baud_domain::{ImageBudget, SixelClaim};

/// detect.go:93, byte for byte, on the payload kitten's loop hands it.
fn kitten_ends_detection(payload: &[u8]) -> bool {
    payload.len() > 3 && payload[0] == b'?' && payload[payload.len() - 1] == b'c'
}

/// The payload kitten's loop hands its CSI handler for `reply`: the bytes
/// after `ESC [`, final byte included. The reply must be one CSI and
/// nothing else, so a second report riding along cannot pass for it.
fn csi_payload(reply: &[u8]) -> &[u8] {
    let p = reply.strip_prefix(b"\x1b[").expect("a seven-bit CSI");
    let end = p
        .iter()
        .position(|b| (0x40..=0x7e).contains(b))
        .expect("a final byte");
    assert_eq!(
        end + 1,
        p.len(),
        "one CSI and nothing after it: {reply:02x?}"
    );
    p
}

/// What `query` answers on a terminal with image memory, after `setup`.
fn reply_to(query: &[u8], setup: impl FnOnce(&mut Term<'_>)) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws).with_images(ImageBudget::of(64));
    let mut t = Term::new(s.storage(), &ws);
    setup(&mut t);
    support::feed(&mut t, query);
    t.reply().to_vec()
}

#[test]
fn kitten_icat_s_detection_ends_on_baud_s_primary_da() {
    // The control: the check can fire. VT102's answer is the one kitten
    // refuses, and a longer one it takes.
    assert!(!kitten_ends_detection(b"?6c"), "VT102 is three bytes");
    assert!(kitten_ends_detection(b"?62c"));

    let default = reply_to(b"\x1b[c", |_| {});
    assert!(
        kitten_ends_detection(csi_payload(&default)),
        "the default answer ends kitten's wait: {default:02x?}"
    );
    assert_eq!(default, b"\x1b[?1;2c", "the default answer");

    let sixel = reply_to(b"\x1b[c", |t| t.set_sixel_claim(SixelClaim::Yes));
    assert!(
        kitten_ends_detection(csi_payload(&sixel)),
        "the Sixel claim ends kitten's wait: {sixel:02x?}"
    );
    assert_eq!(sixel, b"\x1b[?6;4c", "the Sixel claim");

    // DECID answers what DA answers where it is recognised, Level 2 here.
    let decid = reply_to(b"\x1b[62;1\"p\x1bZ", |_| {});
    assert_eq!(decid, default, "DECID at Level 2 answers as DA");
}
