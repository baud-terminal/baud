//! SGR — character attributes.
//!
//! Its own directory because it spans protocol levels: the base attributes are
//! ECMA-48, 256-colour and truecolour are xterm, and the colon sub-parameter
//! form is ISO 8613-6. Filing it under any one of those would misplace the
//! other two.

mod rep_align;
mod subparams;
