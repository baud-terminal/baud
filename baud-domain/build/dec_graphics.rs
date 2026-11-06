//! DEC Special Graphics, `OUT_DIR/dec_graphics.rs`.
//!
//! DEC's tables name each glyph of the set and give no Unicode codepoint:
//! `[[source: vt100-ug-ch3]]` Table 3-9 names all 32 positions, 137
//! through 176 octal, and `[[source: dec-special-graphics]]` draws them.
//! xterm's `map_DEC_Spec_Graphic` (`[[source: xterm-charsets]]`) is one
//! implementation's choice of codepoint for 31 of them, `0x60..0x7E`, and
//! is cited as xterm's, because that is whose judgement it is; foot's
//! `vt100_0[]` (`[[source: foot-vt100-0]]`) makes the same 31 choices,
//! which a test holds it to. The macro leaves `0x5F`, which DEC names
//! "Blank", `UNDEF`; which codepoint that stores is open (`OPEN` in
//! `build/dec_sets.rs`), so the table is 31 entries and baud leaves `0x5F`
//! as `_`.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::dec_sets::{self, CONFORM, ERROR, Set, XTERM_DEPARTS};
use crate::xterm::{self, Digits};
use baud_tablegen::{Check, Emit, Errors, Kind};
use std::fmt::Write as _;

/// The bytes the table covers, `` ` `` through `~`.
pub const FIRST: u8 = 0x60;
pub const COUNT: usize = 31;

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let charsets = xterm::load(read)?;
    let page = dec_sets::code_page(&charsets, "map_DEC_Spec_Graphic", Digits::Either, &mut errs);
    if !errs.is_empty() {
        return Err(errs);
    }
    let xterm = dec_sets::cells(&page);
    // Every position the table holds is one the macro gives a codepoint.
    let table: Vec<u32> = (FIRST..=0x7E)
        .map(|b| {
            let cp = page.entries.get(&b).copied();
            errs.require(Check::from(cp.is_some_and(|cp| cp != ERROR)), || {
                charsets.error(
                    Kind::Control,
                    format!("map_DEC_Spec_Graphic gives 0x{b:02X} no codepoint"),
                )
            });
            cp.unwrap_or(ERROR)
        })
        .collect();
    errs.count(
        charsets.path(),
        "DEC Special Graphics entries",
        table.len(),
        COUNT,
    );
    // Below `` ` `` the table draws the byte: ASCII, and 5/15 open.
    let ours: [u32; 94] = std::array::from_fn(|i| {
        let b = u8::try_from(0x21 + i).expect("a byte");
        if b >= FIRST {
            table[usize::from(b - FIRST)]
        } else {
            u32::from(b)
        }
    });
    for e in dec_sets::named_exactly(Set::Graphics, &xterm, &ours, &CONFORM, &XTERM_DEPARTS) {
        errs.push(charsets.error(Kind::Control, e));
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut out = Emit::new(
        "dec_graphics",
        "baud-domain/build.rs (build/dec_graphics.rs)",
        &[&charsets],
    );
    let mut rs = String::from(
        "/// `[[source: xterm-charsets]]`' `map_DEC_Spec_Graphic`, indexed by\n\
         /// `byte - 0x60`.\n",
    );
    dec_sets::named_docs(&mut rs, Set::Graphics);
    let _ = writeln!(rs, "pub static DEC_GRAPHICS: [u32; {COUNT}] = [");
    for (i, cp) in table.iter().enumerate() {
        let b = char::from(FIRST + u8::try_from(i).expect("a byte"));
        let _ = writeln!(rs, "    0x{cp:04x}, // {b}");
    }
    rs.push_str("];\n");
    out.code(&rs);
    Ok(out)
}
