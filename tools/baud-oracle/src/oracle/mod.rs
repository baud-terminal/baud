//! The eight oracles, one module each, every one the port of the Python
//! script that wrote its file: the same fonts, settings, glyph steps and
//! row order, and the same format, float by float.

pub mod advance;
pub mod bearing;
pub mod cff2;
pub mod component;
pub mod gvar;
pub mod marks;
pub mod var;
pub mod vertical;

use crate::Oracle;

/// Every oracle, in the order the table in the crate's docs gives them.
pub const ALL: [Oracle; 8] = [
    advance::ORACLE,
    bearing::ORACLE,
    cff2::ORACLE,
    component::ORACLE,
    gvar::ORACLE,
    marks::ORACLE,
    var::ORACLE,
    vertical::ORACLE,
];
