//! `SGR 60-65` — the ideogram aspects.
//!
//! `docs/vt-coverage.md` listed these **out**, with the reason "no readable
//! primary source: `[[source: ecma48]]` is a scanned PDF whose text extracts
//! letter-spaced and unsearchable". The PDF is not scanned and the text is not
//! unsearchable. It extracts letter-spaced — `"Operat i ng sy st em"` — so an
//! exact search for `framed` cannot match `f r amed`, and six probes missed for
//! that reason alone. Stripping the spacing from both sides finds every one of
//! them, and § 8.3.117 then defines all six parameters unambiguously.
//!
//! `scripts/extract-ecma48.py` is that search, kept so the claim is
//! reproducible rather than remembered. The lesson is the one this repo already
//! records about instrumentation: a probe that cannot fire looks exactly like a
//! fact, and a MISS is only evidence once a positive control HITs.
//!
//! `Attrs` had three free bits and these need five, so it went to `u32` and
//! `StyleWord` to twenty bytes — `NSTYLE * 4` more, 256 bytes in a 24 MB
//! `Storage`, and nothing on the cell, which holds a `StyleId` index.

use crate::support;
use baud_core::*;
use baud_domain::Attrs;

fn attrs_at(bytes: &[u8]) -> Attrs {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let cell = t.viewport()[0];
    t.style_of(cell).attrs()
}

/// Each of 60-64 sets its own aspect and only its own.
///
/// Table-driven because the failure this guards is a copy-paste one: five
/// near-identical arms where the third sets the second's bit. A `match` per
/// parameter is the shape that silently omits one, which is how `fold` came to
/// discard every attribute except reset and foreground.
#[test]
fn sgr_60_to_64_each_set_one_aspect() {
    let table: [(&[u8], Attrs, &str); 5] = [
        (
            b"\x1b[60mX",
            Attrs::IDEOGRAM_UNDERLINE,
            "60 underline/right",
        ),
        (
            b"\x1b[61mX",
            Attrs::IDEOGRAM_DOUBLE_UNDERLINE,
            "61 double underline/right",
        ),
        (b"\x1b[62mX", Attrs::IDEOGRAM_OVERLINE, "62 overline/left"),
        (
            b"\x1b[63mX",
            Attrs::IDEOGRAM_DOUBLE_OVERLINE,
            "63 double overline/left",
        ),
        (b"\x1b[64mX", Attrs::IDEOGRAM_STRESS, "64 stress marking"),
    ];
    for (bytes, want, what) in table {
        let got = attrs_at(bytes);
        assert!(got.has(want), "{what} is set");
        assert_eq!(
            got.bits(),
            want.bits(),
            "{what} sets that aspect and nothing else"
        );
    }
}

/// `SGR 65` cancels all five at once, which no other parameter does.
///
/// 54 clears two and 23 clears two; 65 clears five. It is the widest
/// multi-bit cancel in SGR, so it is the one most likely to be implemented as
/// a single `without` against the wrong constant.
#[test]
fn sgr_65_cancels_all_five() {
    let all = attrs_at(b"\x1b[60;61;62;63;64mX");
    assert_eq!(all.bits(), Attrs::IDEOGRAM.bits(), "all five accumulate");
    assert_eq!(
        attrs_at(b"\x1b[60;61;62;63;64;65mX"),
        Attrs::NONE,
        "and 65 clears every one"
    );
}

/// 65 clears the ideogram aspects and *only* those.
///
/// The bug this catches is `Unset(!0)` or a mask built from the wrong bit
/// range: a 65 that also switched off bold would pass a test that only ever
/// sent ideogram parameters.
#[test]
fn sgr_65_leaves_the_other_attributes_alone() {
    let got = attrs_at(b"\x1b[1;4;31;60;64;65mX");
    assert!(got.has(Attrs::BOLD), "bold survives 65");
    assert!(got.has(Attrs::UNDERLINE), "underline survives 65");
    assert!(!got.has(Attrs::IDEOGRAM_UNDERLINE), "60 was cancelled");
    assert!(!got.has(Attrs::IDEOGRAM_STRESS), "and so was 64");
}

/// The aspects are independent of the Latin ones that resemble them.
///
/// `SGR 4` underline and `SGR 60` ideogram underline are different aspects,
/// and `SGR 24` must not clear 60 — a reasonable-looking "underline is
/// underline" merge would break CJK rendition and pass every ASCII test.
#[test]
fn ideogram_underline_is_not_latin_underline() {
    assert_ne!(
        attrs_at(b"\x1b[4mX"),
        attrs_at(b"\x1b[60mX"),
        "4 and 60 are two aspects, not one"
    );
    assert!(
        attrs_at(b"\x1b[60;24mX").has(Attrs::IDEOGRAM_UNDERLINE),
        "24 does not cancel 60"
    );
    assert!(
        attrs_at(b"\x1b[4;65mX").has(Attrs::UNDERLINE),
        "and 65 does not cancel 4"
    );
}

/// `SGR 0` resets them, as it resets everything.
#[test]
fn sgr_0_resets_the_ideogram_aspects() {
    assert_eq!(attrs_at(b"\x1b[60;61;62;63;64m\x1b[0mX"), Attrs::NONE);
}

/// 56 and 57 are reserved and unassigned, so they set nothing.
///
/// `[[source: ecma48]]` § 8.3.117 reserves 56-59 for future standardization.
/// baud implements 58 and 59 as the underline colour because
/// `[[source: ctlseqs]]` assigns them that — an extension over ECMA-48, not a
/// reading of it. Nothing assigns 56 or 57, so nothing implements them, and
/// the rule against recording a bit that is never read means they must not
/// quietly land on a neighbouring aspect.
#[test]
fn sgr_56_and_57_are_reserved_and_set_nothing() {
    assert_eq!(attrs_at(b"\x1b[56mX"), Attrs::NONE, "56 is unassigned");
    assert_eq!(attrs_at(b"\x1b[57mX"), Attrs::NONE, "57 is unassigned");
    assert!(
        attrs_at(b"\x1b[1;56;57mX").has(Attrs::BOLD),
        "and neither disturbs a real attribute"
    );
}
