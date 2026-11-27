//! **Whether a read asks for the foreground group**: one answer from three
//! two-state inputs, read before the read is fed, and from no byte.
//!
//! A read asks when the pace is due, when it carries a link (an `OSC 8`
//! introducer or a plain `file://` URI, whose origin is the program that
//! printed it), or when no group is held to file it under. A read inside the
//! pace, carrying no link, with a group held, asks nothing.

/// **Whether the pace asks**: at the first read, or at a read the pace's
/// interval or more after the last one that asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AskPace {
    Due,
    Inside,
}

/// **Whether a read carried a link**: an `OSC 8` introducer or a plain
/// `file://` URI in its text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ReadLinked {
    #[default]
    No,
    Yes,
}

/// **Whether a group is held for the read**: one sampled at an earlier read,
/// or none, at the start or after the shell ended the command it was
/// sampled under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupBefore {
    Named,
    Unnamed,
}

/// **What a read does about the foreground group**: asks nothing, or asks,
/// saying whether it carried a link.
///
/// The answer reads no byte: a byte slice is not an input.
///
/// ```compile_fail,E0308
/// let _ = baud_domain::GroupAsk::decide(
///     b"\x1b]8;",
///     baud_domain::ReadLinked::No,
///     baud_domain::GroupBefore::Named,
/// );
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{AskPace, GroupAsk, GroupBefore, ReadLinked};
/// assert_eq!(
///     GroupAsk::decide(AskPace::Inside, ReadLinked::Yes, GroupBefore::Named),
///     GroupAsk::Yes(ReadLinked::Yes)
/// );
/// assert_eq!(
///     GroupAsk::decide(AskPace::Inside, ReadLinked::No, GroupBefore::Named),
///     GroupAsk::No
/// );
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupAsk {
    No,
    Yes(ReadLinked),
}

impl GroupAsk {
    /// **The answer for one read**: a due pace asks, saying whether the read
    /// is linked; inside the pace a linked read asks, and so does one with
    /// no group held; the rest ask nothing. Every one of the eight inputs
    /// has its own arm.
    pub const fn decide(pace: AskPace, linked: ReadLinked, before: GroupBefore) -> GroupAsk {
        match (pace, linked, before) {
            (AskPace::Due, ReadLinked::No, GroupBefore::Named) => GroupAsk::Yes(ReadLinked::No),
            (AskPace::Due, ReadLinked::No, GroupBefore::Unnamed) => GroupAsk::Yes(ReadLinked::No),
            (AskPace::Due, ReadLinked::Yes, GroupBefore::Named) => GroupAsk::Yes(ReadLinked::Yes),
            (AskPace::Due, ReadLinked::Yes, GroupBefore::Unnamed) => GroupAsk::Yes(ReadLinked::Yes),
            (AskPace::Inside, ReadLinked::Yes, GroupBefore::Named) => {
                GroupAsk::Yes(ReadLinked::Yes)
            }
            (AskPace::Inside, ReadLinked::Yes, GroupBefore::Unnamed) => {
                GroupAsk::Yes(ReadLinked::Yes)
            }
            (AskPace::Inside, ReadLinked::No, GroupBefore::Unnamed) => {
                GroupAsk::Yes(ReadLinked::No)
            }
            (AskPace::Inside, ReadLinked::No, GroupBefore::Named) => GroupAsk::No,
        }
    }
}
