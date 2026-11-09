//! **The display link's one door**: whether a window's view gets a tick at
//! every vsync, or none.
//!
//! A static screen owes no frame, so its link is paused, and it runs again
//! only when something owes a frame or a tick. baud-app decides which (its
//! `frames.rs`); this door is the only place the decision reaches the view.
//! It keeps the state it last sent, so asking for the state the link is
//! already in sends nothing: a pump or a frame that settles on what it
//! found costs no message to AppKit.
//!
//! **What the system pauses, and what this door relies on.** A view's link
//! made by `NSView.displayLink(target:selector:)`, which macos-liquid-glass
//! uses, stops on its own when "the view is hidden, or not on any display"
//! ([[source: apple-nsview-displaylink]]). Whether it ticks for a window
//! that is on screen and wholly covered ([[source: apple-nswindow-occlusionstate]]:
//! "otherwise, the window is fully occluded") is not documented, and this
//! door relies on neither answer: a link it paused stays paused whatever
//! the window's state, and what must happen while no tick comes -- a
//! reply, a deadline's wake, a decode's step -- runs on main-queue turns
//! and wakes, never on a tick.
//!
//! Under the test-only `effect-probe` feature the door records the state
//! instead of sending it (`probe::linked`), so the event rig never makes a
//! display link run, and its own ticks are delivered only while the state
//! recorded is [`LinkState::Run`] (`probe::Ticker`).

#[cfg(not(feature = "effect-probe"))]
use macos_liquid_glass::drawable::DrawableView;
#[cfg(not(feature = "effect-probe"))]
use macos_liquid_glass::objc2::rc::Retained;

/// Whether the view's link delivers ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    /// A tick at every vsync, for as long as something owes one.
    Run,
    /// No ticks: nothing owes a frame, or the window has closed.
    Pause,
}

/// Whether a request reached the backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sent {
    /// The link was already in that state: nothing was sent.
    Nothing,
    /// The state changed, and the backend was told once.
    Once,
}

/// What carries a state change to a link: the view's own display link,
/// the probe's record, or a test's count.
pub trait Backend {
    /// Make the link `to`; called only when the state changes.
    fn send(&self, to: LinkState);
}

/// **What the backend calls for a state**, the one decision both builds
/// share: the view's backend makes the call, and the probe records what
/// it makes of the link. With this crate's calls a run starts a new link
/// and a pause invalidates it, so every run after a pause makes one; a
/// backend that pauses in place changes this table, and the probe then
/// records each run after the first as the one link resumed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Call {
    /// `start_display_link`: a new link, ticking.
    Start,
    /// `stop_display_link`: the link invalidated.
    Stop,
}

/// The call that makes the link `to`.
pub fn call(to: LinkState) -> Call {
    match to {
        LinkState::Run => Call::Start,
        LinkState::Pause => Call::Stop,
    }
}

/// What a call makes of the view's link.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Making {
    /// A new link.
    New,
    /// The link it has, or none.
    Same,
}

impl Call {
    /// What this call makes of the view's link.
    pub fn making(self) -> Making {
        match self {
            Call::Start => Making::New,
            Call::Stop => Making::Same,
        }
    }
}

/// **The door**: the state last sent, and the backend that carries a
/// change. Starts paused, as a view's link is before its window opens.
pub struct Link<B: Backend> {
    backend: B,
    state: LinkState,
}

impl<B: Backend> Link<B> {
    /// A paused link over `backend`.
    pub fn new(backend: B) -> Link<B> {
        Link {
            backend,
            state: LinkState::Pause,
        }
    }

    /// **The link made `to`**: the backend told only when that changes
    /// the state, so asking for the state the link is in sends nothing.
    pub fn set(&mut self, to: LinkState) -> Sent {
        if self.state == to {
            return Sent::Nothing;
        }
        self.state = to;
        self.backend.send(to);
        Sent::Once
    }

    /// The state last sent.
    pub fn state(&self) -> LinkState {
        self.state
    }
}

/// **A window's view, as the door's backend**: its display link started
/// and stopped (macos-liquid-glass's `start_display_link`, which makes a
/// new link, and `stop_display_link`, which invalidates it). Under the
/// probe it records the state and holds no view.
pub struct ViewLink {
    #[cfg(not(feature = "effect-probe"))]
    view: Retained<DrawableView>,
}

impl ViewLink {
    /// The backend for `view`'s link.
    #[cfg(not(feature = "effect-probe"))]
    pub fn of(view: &DrawableView) -> ViewLink {
        use macos_liquid_glass::objc2::Message;
        ViewLink {
            view: view.retain(),
        }
    }

    /// The backend for `view`'s link: under the probe, a record and no
    /// view.
    #[cfg(feature = "effect-probe")]
    pub fn of(_view: &macos_liquid_glass::drawable::DrawableView) -> ViewLink {
        ViewLink {}
    }
}

/// **A closing window's link stopped**, whatever its door last sent: the
/// door lives in the window's driver, which the close takes down a turn
/// later, and a link left running would tick a view with no responder.
pub fn closed(view: &macos_liquid_glass::drawable::DrawableView) {
    ViewLink::of(view).send(LinkState::Pause);
}

impl Backend for ViewLink {
    #[cfg(not(feature = "effect-probe"))]
    fn send(&self, to: LinkState) {
        match call(to) {
            Call::Start => self.view.start_display_link(),
            Call::Stop => self.view.stop_display_link(),
        }
    }

    #[cfg(feature = "effect-probe")]
    fn send(&self, to: LinkState) {
        // What the real backend's call makes of the link, from the one
        // table both builds read.
        crate::probe::linked(to, call(to).making());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;

    /// A backend that keeps what it was sent.
    struct Counted<'a>(&'a RefCell<Vec<LinkState>>);

    impl Backend for Counted<'_> {
        fn send(&self, to: LinkState) {
            self.0.borrow_mut().push(to);
        }
    }

    /// **A state already in force sends nothing**: run, run, pause, pause,
    /// run is three sends, each a change, and a fresh link is paused, so a
    /// first pause sends nothing either. Break: `set` without its compare,
    /// and the repeats send.
    #[test]
    fn a_state_already_in_force_sends_nothing() {
        let sent = RefCell::new(Vec::new());
        let mut link = Link::new(Counted(&sent));
        assert_eq!(link.state(), LinkState::Pause, "a fresh link is paused");
        let asked = [
            (LinkState::Pause, Sent::Nothing),
            (LinkState::Run, Sent::Once),
            (LinkState::Run, Sent::Nothing),
            (LinkState::Pause, Sent::Once),
            (LinkState::Pause, Sent::Nothing),
            (LinkState::Run, Sent::Once),
        ];
        for (to, want) in asked {
            assert_eq!(link.set(to), want, "{to:?}");
            assert_eq!(link.state(), to);
        }
        assert_eq!(
            *sent.borrow(),
            [LinkState::Run, LinkState::Pause, LinkState::Run],
            "one send a change"
        );
    }

    /// **The calls say what they make**: a run starts a new link and a
    /// pause invalidates it, the table the view's backend sends and the
    /// probe records, so the one-link row reads the backend's own answer.
    /// Break: the run resuming, and the row passes while this crate's calls
    /// still make a link each run.
    #[test]
    fn each_call_says_what_it_makes_of_the_link() {
        assert_eq!(call(LinkState::Run), Call::Start);
        assert_eq!(call(LinkState::Pause), Call::Stop);
        assert_eq!(Call::Start.making(), Making::New);
        assert_eq!(Call::Stop.making(), Making::Same);
    }
}
