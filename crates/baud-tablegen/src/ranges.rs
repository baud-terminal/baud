//! A binary property as the ranges of code points that have it.

use crate::{CpRange, Errors, Kind, Source};

/// The code points with binary `property` in a file whose rows are
/// `<range> ; <property>` among rows of other properties, such as
/// DerivedCoreProperties.txt (`[[source: dcp-data]]`): sorted, and
/// adjacent rows merged, so the ranges are as few as the set allows. A row
/// of `property` with a field past the two, and two of its rows over one
/// code point, are refused.
pub fn binary_ranges(src: &Source, property: &str) -> Result<Vec<CpRange>, Errors> {
    let mut errs = Errors::default();
    let mut rows = Vec::new();
    for row in src.rows().filter(|r| r.field(1) == property) {
        if let Some(r) = errs.take(row.expect(2).and_then(|()| row.range(0))) {
            rows.push((r, row));
        }
    }
    rows.sort_by_key(|(r, _)| r.lo);
    let mut out: Vec<CpRange> = Vec::new();
    let mut last: Option<usize> = None;
    for (r, row) in &rows {
        if let (Some(prev), Some(line)) = (out.last_mut(), last) {
            if r.lo <= prev.hi {
                errs.push(row.error(
                    Kind::Overlap,
                    0,
                    format!(
                        "U+{:04X} is already given {property} by the rows up to line {line} ({:?})",
                        r.lo,
                        src.line(line)
                    ),
                ));
                continue;
            }
            if r.lo == prev.hi + 1 {
                prev.hi = r.hi;
                last = Some(row.line());
                continue;
            }
        }
        out.push(*r);
        last = Some(row.line());
    }
    errs.or(out)
}
