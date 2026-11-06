//! The three calls per chunk, and what counts as the cycle being broken.

use crate::Error;

/// The buffer protocol the kitty row drives. `python::Screen` is the real one;
/// the tests here hold fakes that misbehave.
pub trait Cycle {
    type Dest;
    /// `Screen.test_create_write_buffer`
    fn create(&mut self) -> Result<Self::Dest, Error>;
    /// `Screen.test_commit_write_buffer`, returning the bytes it took.
    fn commit(&mut self, data: &[u8], dest: &Self::Dest) -> Result<usize, Error>;
    /// `Screen.test_parse_written_data`
    fn parse(&mut self) -> Result<(), Error>;
}

/// Give all of `chunk` to the parser, returning the bytes it took.
///
/// A commit of no bytes is a failure and not another turn of the loop: with
/// nothing consumed the loop would never end.
pub fn feed<C: Cycle>(cycle: &mut C, chunk: &[u8]) -> Result<usize, Error> {
    let mut rest = chunk;
    while !rest.is_empty() {
        let dest = cycle.create()?;
        let n = cycle.commit(rest, &dest)?;
        if n == 0 || n > rest.len() {
            return Err(Error::BrokenCycle {
                offered: rest.len(),
                committed: n,
            });
        }
        rest = &rest[n..];
        cycle.parse()?;
    }
    Ok(chunk.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Takes `take` bytes a commit and records the calls in order.
    struct Fake {
        take: usize,
        calls: Vec<&'static str>,
        taken: Vec<u8>,
    }

    impl Fake {
        fn taking(take: usize) -> Fake {
            Fake {
                take,
                calls: Vec::new(),
                taken: Vec::new(),
            }
        }
    }

    impl Cycle for Fake {
        type Dest = ();
        fn create(&mut self) -> Result<(), Error> {
            self.calls.push("create");
            Ok(())
        }
        fn commit(&mut self, data: &[u8], _dest: &()) -> Result<usize, Error> {
            self.calls.push("commit");
            let n = self.take.min(data.len());
            self.taken.extend_from_slice(&data[..n]);
            Ok(n)
        }
        fn parse(&mut self) -> Result<(), Error> {
            self.calls.push("parse");
            Ok(())
        }
    }

    #[test]
    fn a_whole_commit_is_one_cycle_in_order() {
        let mut f = Fake::taking(4096);
        assert_eq!(feed(&mut f, b"abc").unwrap(), 3);
        assert_eq!(f.calls, ["create", "commit", "parse"]);
        assert_eq!(f.taken, b"abc");
    }

    #[test]
    fn a_partial_commit_takes_another_cycle_from_where_it_stopped() {
        let mut f = Fake::taking(2);
        assert_eq!(feed(&mut f, b"abcde").unwrap(), 5);
        assert_eq!(f.taken, b"abcde");
        assert_eq!(f.calls.len(), 9, "{:?}", f.calls);
    }

    #[test]
    fn an_empty_chunk_makes_no_call() {
        let mut f = Fake::taking(1);
        assert_eq!(feed(&mut f, b"").unwrap(), 0);
        assert!(f.calls.is_empty());
    }

    #[test]
    fn a_commit_of_nothing_is_a_broken_cycle_and_not_a_hang() {
        let e = feed(&mut Fake::taking(0), b"abc").unwrap_err();
        assert!(
            matches!(
                e,
                Error::BrokenCycle {
                    offered: 3,
                    committed: 0
                }
            ),
            "{e:?}"
        );
    }

    struct Overclaims;
    impl Cycle for Overclaims {
        type Dest = ();
        fn create(&mut self) -> Result<(), Error> {
            Ok(())
        }
        fn commit(&mut self, data: &[u8], _dest: &()) -> Result<usize, Error> {
            Ok(data.len() + 1)
        }
        fn parse(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn a_commit_of_more_than_was_offered_is_a_broken_cycle() {
        let e = feed(&mut Overclaims, b"abc").unwrap_err();
        assert!(
            matches!(
                e,
                Error::BrokenCycle {
                    offered: 3,
                    committed: 4
                }
            ),
            "{e:?}"
        );
    }

    struct Fails(&'static str);
    impl Fails {
        fn at(&self, call: &str) -> Result<(), Error> {
            if self.0 == call {
                return Err(Error::Python { call: call.into() });
            }
            Ok(())
        }
    }
    impl Cycle for Fails {
        type Dest = ();
        fn create(&mut self) -> Result<(), Error> {
            self.at("create")
        }
        fn commit(&mut self, data: &[u8], _dest: &()) -> Result<usize, Error> {
            self.at("commit").map(|()| data.len())
        }
        fn parse(&mut self) -> Result<(), Error> {
            self.at("parse")
        }
    }

    #[test]
    fn a_failure_at_any_of_the_three_calls_comes_back() {
        for which in ["create", "commit", "parse"] {
            let e = feed(&mut Fails(which), b"abc").unwrap_err();
            assert!(
                matches!(&e, Error::Python { call } if call == which),
                "{which}: {e:?}"
            );
        }
    }
}
