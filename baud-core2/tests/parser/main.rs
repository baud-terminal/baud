//! The parser itself — states, and the properties that must hold for any input.
//!
//! Below every protocol rather than beside them: a state-machine bug is wrong
//! for all of them at once, and chunk-invariance is the property the whole
//! design rests on.

mod properties;
mod states;
mod vt_properties;
