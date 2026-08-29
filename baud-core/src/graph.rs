//! Where the parser stands, and what one byte does from there.
//!
//! The block loop takes runs; this takes what a run cannot. A byte that is not
//! the start of a printable run, and a character split across two `read(2)`s,
//! both land here.
//!
//! **The node carries no payload.** A UTF-8 accumulator is a value, not a
//! state, so the continuation count *is* the node -- `Utf8One`, `Utf8Two`,
//! `Utf8Three` -- and the bits live beside it on `Term`. That is what lets the
//! node be an index; a payload cannot be one.

use baud_domain::*;

/// A node of the parse graph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Node {
    /// Between sequences, where a run or a character may start.
    Ground = 0,
    /// Mid-character, with this many continuation bytes still owed.
    Utf8One,
    Utf8Two,
    Utf8Three,
}

impl Node {
    /// Is the parser between sequences? The block loop only takes a block from
    /// here, because a run cannot start inside one.
    #[inline(always)]
    pub(crate) fn is_ground(self) -> bool {
        matches!(self, Node::Ground)
    }

    /// The node for a lead byte owing `n` continuations.
    #[inline(always)]
    pub(crate) fn owing(n: Pending) -> Node {
        match n.owed() {
            3 => Node::Utf8Three,
            2 => Node::Utf8Two,
            _ => Node::Utf8One,
        }
    }

    /// One continuation nearer the end.
    #[inline(always)]
    pub(crate) fn one_fewer(self) -> Node {
        match self {
            Node::Utf8Three => Node::Utf8Two,
            Node::Utf8Two => Node::Utf8One,
            _ => Node::Ground,
        }
    }
}
