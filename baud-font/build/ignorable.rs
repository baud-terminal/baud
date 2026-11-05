//! The default-ignorable table, `OUT_DIR/ignorable.rs`:
//! `Default_Ignorable_Code_Point` (`[[source: dcp-data]]`), the set a
//! shaper skips while matching unless a pattern names the glyph itself --
//! ZWJ, ZWNJ, variation selectors, tags and the like -- as sorted,
//! inclusive, non-overlapping ranges for a binary search. baud-domain's
//! build compiles the same ranges from the same file through the same
//! `baud_tablegen::binary_ranges`, for the core.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{Check, CpRange, Emit, Errors, Input, Kind, Source, binary_ranges};
use std::fmt::Write as _;

pub const CORE_PROPERTIES: Input = Input {
    file: "unicode-derivedcoreproperties.txt",
    key: "dcp-data",
    header: "# DerivedCoreProperties-17.0.0.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 1] = [CORE_PROPERTIES];

/// The ranges the file's 27 rows merge to.
const RANGES: usize = 17;

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let dcp = Source::load(&CORE_PROPERTIES, read)?;
    let ranges = binary_ranges(&dcp, "Default_Ignorable_Code_Point")?;
    errs.count(
        dcp.path(),
        "Default_Ignorable_Code_Point ranges",
        ranges.len(),
        RANGES,
    );
    let has = |cp: u32| ranges.iter().any(|r: &CpRange| r.lo <= cp && cp <= r.hi);
    // Positive controls: ZWJ, VS16 and a tag in; a Latin letter and an
    // emoji out.
    for (cp, want) in [
        (0x200D, true),
        (0xFE0F, true),
        (0xE0067, true),
        (0x0041, false),
        (0x1F600, false),
    ] {
        errs.require(Check::from(has(cp) == want), || {
            dcp.error(
                Kind::Control,
                format!(
                    "U+{cp:04X} is default-ignorable: {}, {want} expected",
                    has(cp)
                ),
            )
        });
    }
    let mut out = Emit::new(
        "ignorable",
        "baud-font/build.rs (build/ignorable.rs)",
        &[&dcp],
    );
    out.code(&rust(&ranges));
    errs.or(out)
}

/// `DEFAULT_IGNORABLE`, a const slice of the ranges.
pub fn rust(ranges: &[CpRange]) -> String {
    let mut rs = String::from(
        "/// Sorted, inclusive, non-overlapping ranges.\n\
         pub const DEFAULT_IGNORABLE: &[(u32, u32)] = &[\n",
    );
    for r in ranges {
        let _ = writeln!(rs, "    (0x{:04X}, 0x{:04X}),", r.lo, r.hi);
    }
    rs.push_str("];\n");
    rs
}
