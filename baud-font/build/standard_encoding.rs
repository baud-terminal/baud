//! The CFF Standard Encoding, `OUT_DIR/standard_encoding.rs`: code to
//! SID, the table `seac` names its two glyphs through. It is Appendix B,
//! "Predefined Encodings", of `[[source: adobe-5176-cff]]`, whose code and
//! SID columns `data/adobe-5176-cff-standard-encoding.txt` holds as read
//! from the PDF once (`[[source: adobe-5176-cff-standard-encoding]]`); the
//! file's header carries the attribution its licence asks for.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{Check, Emit, Errors, Input, Kind, Source};
use std::fmt::Write as _;

pub const STANDARD_ENCODING: Input = Input {
    file: "adobe-5176-cff-standard-encoding.txt",
    key: "adobe-5176-cff-standard-encoding",
    header: "# CFF Standard Encoding, code; SID, from adobe-5176-cff.pdf sha256 \
             60e0696c4fac326de597722af37bcdbbd688e0337a548709a287405ff3b3996a",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 1] = [STANDARD_ENCODING];

/// The codes an encoding has: one byte's.
pub const CODES: usize = 256;

/// Codes and the SIDs the appendix gives them, read from its own rows:
/// space, `A`, `a`, `.notdef` at 0 and `germandbls`, the last SID.
pub const CONTROLS: [(usize, u8); 5] = [(32, 1), (65, 34), (97, 66), (0, 0), (251, 149)];

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let src = Source::load(&STANDARD_ENCODING, read)?;
    let mut table: Vec<u8> = Vec::with_capacity(CODES);
    for row in src.rows() {
        if errs.take(row.expect(2)).is_none() {
            continue;
        }
        let decimal = |i: usize| {
            let f = row.field(i);
            f.parse::<u32>()
                .ok()
                .filter(|_| f.bytes().all(|b| b.is_ascii_digit()))
                .ok_or_else(|| {
                    row.error(Kind::Malformed, i, format!("{f:?} is not a decimal number"))
                })
        };
        let (Some(code), Some(sid)) = (errs.take(decimal(0)), errs.take(decimal(1))) else {
            continue;
        };
        if usize::try_from(code).ok() != Some(table.len()) {
            errs.push(row.error(
                Kind::Invariant,
                0,
                format!("code {code} where code {} is next", table.len()),
            ));
            continue;
        }
        let Some(sid) = errs.take(u8::try_from(sid).map_err(|_| {
            row.error(
                Kind::Invariant,
                1,
                format!("SID {sid} does not fit the byte the table stores it in"),
            )
        })) else {
            continue;
        };
        table.push(sid);
    }
    errs.count(src.path(), "codes", table.len(), CODES);
    for (code, sid) in CONTROLS {
        let got = table.get(code).copied();
        errs.require(Check::from(got == Some(sid)), || {
            src.error(
                Kind::Control,
                format!("code {code} has SID {got:?}, {sid} expected"),
            )
        });
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut out = Emit::new(
        "standard_encoding",
        "baud-font/build.rs (build/standard_encoding.rs)",
        &[&src],
    );
    out.code(&rust(&table));
    errs.or(out)
}

/// `STANDARD_ENCODING`, a const byte string of the SIDs in code order.
pub fn rust(table: &[u8]) -> String {
    let mut rs = String::from(
        "/// `STANDARD_ENCODING[code]` is the SID; 0 is `.notdef`.\n\
         pub const STANDARD_ENCODING: &[u8; 256] = b\"",
    );
    for b in table {
        let _ = write!(rs, "\\x{b:02x}");
    }
    rs.push_str("\";\n");
    rs
}
