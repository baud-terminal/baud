//! **The Noto instances, through baud's own bake.** `baud-fontbuild
//! instance VF.ttf WGHT EPOCH OUT.ttf` writes `VF.ttf` with `wght` pinned
//! at `WGHT` and every other axis kept, `head.modified` stamped from the
//! Unix time `EPOCH` as fontTools stamps it from `SOURCE_DATE_EPOCH`: the
//! partial instance `baud_font::bake_partial` writes, which
//! `tests/bake.rs` holds equal, under [`crate::compare`], to fontTools
//! 4.63.0's instances of Noto Sans Devanagari at 400 and 700.

use baud_font::{BakePoint, IupStep, Modified, Refused, bake_partial};

/// Seconds from 1904-01-01 to 1970-01-01, the epochs of `LONGDATETIME`
/// and of Unix time.
const EPOCH_1904: i64 = 2_082_844_800;

/// `font` with `wght` pinned at `wght` (user units) and `head.modified`
/// at the Unix time `epoch`.
pub fn instance(font: &[u8], wght: i32, epoch: i64) -> Result<Vec<u8>, String> {
    let modified = Modified(epoch.checked_add(EPOCH_1904).ok_or("the epoch overflows")?);
    let wght = baud_font::var::Fixed(wght.checked_mul(1 << 16).ok_or("the weight overflows")?);
    let mut scratch = vec![BakePoint::ZERO; 1 << 16];
    let mut steps = vec![IupStep::ZERO; 1 << 17];
    let mut out = vec![0u8; font.len() * 2];
    let len = loop {
        match bake_partial(font, wght, modified, &mut scratch, &mut steps, &mut out) {
            Ok(len) => break len,
            Err(Refused::Out(need)) => out = vec![0u8; need.value() as usize],
            Err(e) => return Err(format!("{e:?}")),
        }
    };
    out.truncate(len.value() as usize);
    Ok(out)
}
