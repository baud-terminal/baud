//! `GroupAsk::decide`: whether a read asks for the foreground group, over
//! every one of its eight inputs.

use baud_domain::{AskPace, GroupAsk, GroupBefore, ReadLinked};

/// **Each of the eight inputs gets its own answer, and the table is
/// whole**: a due pace asks and carries the read's link verdict; inside the
/// pace a linked read asks with `Yes`, an unlinked one asks with `No` only
/// when no group is held, and asks nothing when one is. The inputs are
/// enumerated, not sampled, and the count of them is checked.
/// Named break: answer `No` for a due pace with a group held -- the pace's
/// sample is lost and the case `(Due, No, Named)` fails; answer by the link
/// alone -- `(Inside, No, Unnamed)` fails.
#[test]
fn a_read_asks_by_the_eight_case_table() {
    let paces = [AskPace::Due, AskPace::Inside];
    let links = [ReadLinked::No, ReadLinked::Yes];
    let befores = [GroupBefore::Named, GroupBefore::Unnamed];
    let mut seen = 0;
    for pace in paces {
        for linked in links {
            for before in befores {
                let want = match (pace, linked, before) {
                    (AskPace::Due, ReadLinked::No, GroupBefore::Named) => {
                        GroupAsk::Yes(ReadLinked::No)
                    }
                    (AskPace::Due, ReadLinked::No, GroupBefore::Unnamed) => {
                        GroupAsk::Yes(ReadLinked::No)
                    }
                    (AskPace::Due, ReadLinked::Yes, GroupBefore::Named) => {
                        GroupAsk::Yes(ReadLinked::Yes)
                    }
                    (AskPace::Due, ReadLinked::Yes, GroupBefore::Unnamed) => {
                        GroupAsk::Yes(ReadLinked::Yes)
                    }
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
                };
                assert_eq!(
                    GroupAsk::decide(pace, linked, before),
                    want,
                    "{pace:?} {linked:?} {before:?}"
                );
                seen += 1;
            }
        }
    }
    assert_eq!(seen, 8, "every input");
}

/// **A linked read always asks, and says it is linked**, whatever the pace
/// and the group held: the sample a link's origin is taken from is never
/// skipped. Named break: let a held group stand in for a linked read
/// inside the pace -- `(Inside, Yes, Named)` asks nothing.
#[test]
fn a_linked_read_always_asks_as_linked() {
    for pace in [AskPace::Due, AskPace::Inside] {
        for before in [GroupBefore::Named, GroupBefore::Unnamed] {
            assert_eq!(
                GroupAsk::decide(pace, ReadLinked::Yes, before),
                GroupAsk::Yes(ReadLinked::Yes),
                "{pace:?} {before:?}"
            );
        }
    }
}
