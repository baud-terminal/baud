//! **Where a link goes, before it is opened**: what a frame draws while
//! the link gesture's modifier is held over a link -- the link's cells
//! underlined and its address in a bar of its own -- for every path.
//!
//! The driver reads the link, its cells and the address it shows
//! (`baud-app`'s `link.rs`, which escapes it); this only draws them: the
//! underline as solids, the bar as a field drawn over its cells in the
//! selection's colours, the find field's way ([`Field`]).

use baud_domain::{Col, Row};

use crate::find::Field;

/// One row's run of a link's cells, both ends inclusive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkSpan {
    pub row: Row,
    pub from: Col,
    pub to: Col,
}

/// **What the link preview draws on the next frame**: the link's cells on
/// the view, a run a row, underlined, and the bar with its address.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkLook {
    pub spans: Vec<LinkSpan>,
    pub bar: Field,
}
