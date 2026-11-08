//! **What a handler asked of the OS, held for the loop's next turn.**
//!
//! Linux's driver runs in a poll loop, and a handler -- a pointer press, a
//! key, the printed paths worker's answer -- runs inside the loop's body.
//! An effect it asks for (a reveal over the session bus, a command, a
//! copy) is pushed here, and the loop takes the batch at the top of its
//! next iteration, after `dispatch` has returned and with no handler on
//! the stack ([`NextTurn::take`], [`NextTurn::keep`]). The macOS driver's
//! main-queue turn is the same rule on a run loop (`app.rs`, `Turned`).
//!
//! The storage is kept from turn to turn, so a turn with nothing asked
//! allocates nothing.

/// The effects asked for this turn, in order.
pub(crate) struct NextTurn<E>(Vec<E>);

impl<E> Default for NextTurn<E> {
    fn default() -> NextTurn<E> {
        NextTurn(Vec::new())
    }
}

impl<E> NextTurn<E> {
    /// Ask for `effect` on the next turn: nothing runs now.
    pub(crate) fn push(&mut self, effect: E) {
        self.0.push(effect);
    }

    /// The batch to run now, in order; what is pushed while it runs waits
    /// for the turn after. Hand the batch back with [`NextTurn::keep`].
    pub(crate) fn take(&mut self) -> Vec<E> {
        core::mem::take(&mut self.0)
    }

    /// The batch's storage, emptied, back for the next turn, with anything
    /// pushed meanwhile kept after it.
    pub(crate) fn keep(&mut self, mut batch: Vec<E>) {
        batch.clear();
        batch.append(&mut self.0);
        self.0 = batch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sink that counts what ran, and where: a handler pushes, and only
    /// the turn runs.
    #[derive(Default)]
    struct Sink {
        queue: NextTurn<char>,
        ran: Vec<(char, Phase)>,
        phase: Phase,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    enum Phase {
        #[default]
        Handler,
        Turn,
    }

    impl Sink {
        /// A handler: asks, and runs nothing.
        fn handle(&mut self, e: char) {
            self.phase = Phase::Handler;
            self.queue.push(e);
        }

        /// The loop's turn, after the handlers returned: each asked effect
        /// once, in order; one that asks for another leaves it a turn.
        fn turn(&mut self) {
            self.phase = Phase::Turn;
            let mut batch = self.queue.take();
            for e in batch.drain(..) {
                self.ran.push((e, self.phase));
                if e == 'a' {
                    self.queue.push('z');
                }
            }
            self.queue.keep(batch);
        }
    }

    /// **An effect runs once, on the turn, never inside its handler.**
    /// Perturbation: run the effect in `handle` and the first assertion
    /// fails; drop `keep`'s append and the one asked for during a turn is
    /// lost.
    #[test]
    fn an_effect_runs_once_on_the_turn_after_its_handler() {
        let mut s = Sink::default();
        s.handle('a');
        s.handle('b');
        assert!(s.ran.is_empty(), "nothing inside the handlers");
        s.turn();
        assert_eq!(s.ran, [('a', Phase::Turn), ('b', Phase::Turn)]);
        s.turn();
        assert_eq!(
            s.ran,
            [('a', Phase::Turn), ('b', Phase::Turn), ('z', Phase::Turn)],
            "each once; what a turn asks for runs on the next"
        );
        s.turn();
        assert_eq!(s.ran.len(), 3, "and nothing again");
    }

    /// **The storage is kept**: a turn hands it back, so the next push
    /// reuses it rather than allocating.
    #[test]
    fn a_turn_keeps_the_storage() {
        let mut q: NextTurn<u8> = NextTurn::default();
        q.push(1);
        q.push(2);
        let batch = q.take();
        let at = batch.as_ptr();
        q.keep(batch);
        q.push(3);
        let again = q.take();
        assert_eq!((again.as_ptr(), again.as_slice()), (at, &[3][..]));
    }

    /// **Linux's driver uses it so**: a printed path's three effects are
    /// pushed, never made, in `path_checked`, and the loop runs the batch
    /// at the top of each iteration, before it waits, so the handler that
    /// asked has returned and the effect does not wait on the next event.
    /// The positions are the control. Perturbation: put `bus.reveal(` back
    /// in `path_checked` and the second assertion fails.
    #[test]
    fn linux_runs_a_printed_paths_effects_on_the_next_turn() {
        let src = include_str!("linux.rs");
        let body = src
            .split_once(concat!(
                "    fn path_",
                "checked(&mut self, done: crate::probe::Clicked) {"
            ))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(b, _)| b)
            .expect("path_checked");
        for asked in ["Later::Reveal(", "Later::Command(cmd)", "Later::Copy(text)"] {
            assert!(body.contains(asked), "{asked} pushed: {body}");
        }
        for made in ["bus.reveal(", "opener.start(", ".link_copied("] {
            assert!(!body.contains(made), "{made} made in the handler");
        }
        let run = src.split_once("'run: loop {").expect("the loop").1;
        let at = run.find(concat!("d.run_", "later();")).expect("the turn");
        let wait = run.find("let waited = wait(").expect("the wait");
        let dispatch = run.find(".dispatch(&mut d.window)").expect("the dispatch");
        assert!(
            at < wait && wait < dispatch,
            "taken at the top, before the wait"
        );
    }
}
