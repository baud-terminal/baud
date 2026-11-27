//! **A toplevel's decoration negotiation**, `xdg-decoration-unstable-v1`
//! (`[[source: wp-xdg-decoration-unstable-v1]]`), as types: which mode the
//! window asks for, when it may ask, which mode it takes, and when it may
//! attach a buffer. The presenter sends and receives the messages; every
//! rule about them is here.
//!
//! - The window asks once, for server-side decorations. "Clients ... must
//!   make sure not to send multiple successive set_mode requests with the
//!   same decoration mode", so [`Negotiation::ask`] answers once.
//! - At manager version 1, "creating an xdg_toplevel_decoration from an
//!   xdg_toplevel which has a buffer attached or committed is a client
//!   error". Version 2 allows it. [`Negotiation::ask`] takes the toplevel's
//!   state and the version, and refuses the late ask at version 1.
//! - "The configured state should not be applied immediately": a configure
//!   is held until the `xdg_surface` configure that follows it is
//!   acknowledged ([`Negotiation::acked`]). "The specified mode must be
//!   obeyed by the client", so the mode configured is the mode taken.
//! - At version 1, "any attempts by a client to attach or manipulate a
//!   buffer prior to the first xdg_toplevel_decoration.configure event must
//!   also be treated as errors" ([`Negotiation::may_attach`]).
//! - A mode outside the protocol's two is not a mode
//!   ([`DecorationMode::try_from`]); the window keeps the one it has.

/// **A decoration mode**: whether the compositor draws the window's
/// decorations. The discriminants are the protocol's wire values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DecorationMode {
    /// "no server-side window decoration".
    #[default]
    ClientSide = 1,
    /// "server-side window decoration": the compositor's titlebar carries
    /// the window's title.
    ServerSide = 2,
}

impl DecorationMode {
    /// The mode's value on the wire.
    pub const fn wire(self) -> u32 {
        self as u32
    }
}

/// **A wire value that names no decoration mode**, kept as it arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidMode(pub u32);

impl TryFrom<u32> for DecorationMode {
    type Error = InvalidMode;

    fn try_from(wire: u32) -> Result<Self, InvalidMode> {
        match wire {
            1 => Ok(DecorationMode::ClientSide),
            2 => Ok(DecorationMode::ServerSide),
            other => Err(InvalidMode(other)),
        }
    }
}

/// **The decoration manager's version, as bound**: the window binds 2 at
/// most, and version 2 is the first to allow a decoration for a toplevel
/// that has committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagerVersion {
    V1,
    V2,
}

impl ManagerVersion {
    /// The version to bind for a manager the compositor advertises at
    /// `advertised`; none for 0, which no compositor may advertise.
    pub const fn bound(advertised: u32) -> Option<ManagerVersion> {
        match advertised {
            0 => None,
            1 => Some(ManagerVersion::V1),
            _ => Some(ManagerVersion::V2),
        }
    }

    /// The version's value on the wire.
    pub const fn wire(self) -> u32 {
        match self {
            ManagerVersion::V1 => 1,
            ManagerVersion::V2 => 2,
        }
    }
}

/// **Whether the toplevel has committed**: its first `wl_surface.commit`
/// is what makes a decoration "from an xdg_toplevel which has a buffer
/// attached or committed".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toplevel {
    Uncommitted,
    Committed,
}

/// **The one ask the window may send**: `get_toplevel_decoration`, then
/// `set_mode` with [`AskServerSide::MODE`]. Only [`Negotiation::ask`] makes
/// one, so the requests cannot be sent twice or at a version that forbids
/// them.
///
/// ```compile_fail,E0451
/// let _ = baud_domain::AskServerSide { _made_by_ask: () };
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct AskServerSide {
    _made_by_ask: (),
}

impl AskServerSide {
    /// The mode the window asks for.
    pub const MODE: DecorationMode = DecorationMode::ServerSide;
}

/// Whether the window has asked, and at which manager version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    No,
    At(ManagerVersion),
}

/// Whether a decoration configure has arrived after the ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Heard {
    Nothing,
    Configured,
}

/// **One toplevel's negotiation**: what it asked, the mode a configure
/// carried and not yet acknowledged, and the mode it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Negotiation {
    asked: Asked,
    heard: Heard,
    pending: Option<DecorationMode>,
    mode: DecorationMode,
}

impl Default for Negotiation {
    fn default() -> Self {
        Negotiation::new()
    }
}

impl Negotiation {
    /// A toplevel that has asked nothing: its decorations are its own, as
    /// every toplevel's are where the protocol is not negotiated.
    pub const fn new() -> Negotiation {
        Negotiation {
            asked: Asked::No,
            heard: Heard::Nothing,
            pending: None,
            mode: DecorationMode::ClientSide,
        }
    }

    /// **The ask, if the window may make it now**: once, and at version 1
    /// only before the toplevel's first commit.
    pub fn ask(&mut self, version: ManagerVersion, toplevel: Toplevel) -> Option<AskServerSide> {
        match (self.asked, version, toplevel) {
            (Asked::At(_), _, _) => None,
            (Asked::No, ManagerVersion::V1, Toplevel::Committed) => None,
            (Asked::No, _, _) => {
                self.asked = Asked::At(version);
                Some(AskServerSide { _made_by_ask: () })
            }
        }
    }

    /// **A decoration configure**, held until the `xdg_surface` configure
    /// after it is acknowledged. A value naming no mode leaves what is held
    /// as it was.
    pub fn configured(&mut self, wire: u32) -> Result<DecorationMode, InvalidMode> {
        let mode = DecorationMode::try_from(wire)?;
        self.heard = Heard::Configured;
        self.pending = Some(mode);
        Ok(mode)
    }

    /// **The `xdg_surface` configure was acknowledged**: the mode a
    /// decoration configure carried before it is the window's.
    pub fn acked(&mut self) -> DecorationMode {
        if let Some(mode) = self.pending.take() {
            self.mode = mode;
        }
        self.mode
    }

    /// The mode the window has.
    pub const fn mode(&self) -> DecorationMode {
        self.mode
    }

    /// **Whether a buffer may be attached**: not after an ask at version 1
    /// until a decoration configure has arrived.
    pub const fn may_attach(&self) -> bool {
        !matches!(
            (self.asked, self.heard),
            (Asked::At(ManagerVersion::V1), Heard::Nothing)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The wire values are the protocol's**, and no other value is a
    /// mode.
    #[test]
    fn a_mode_is_one_of_the_protocol_s_two() {
        assert_eq!(DecorationMode::ClientSide.wire(), 1);
        assert_eq!(DecorationMode::ServerSide.wire(), 2);
        for wire in [0, 3, 4, u32::MAX] {
            assert_eq!(DecorationMode::try_from(wire), Err(InvalidMode(wire)));
        }
        assert_eq!(DecorationMode::try_from(1), Ok(DecorationMode::ClientSide));
        assert_eq!(DecorationMode::try_from(2), Ok(DecorationMode::ServerSide));
    }

    /// **The window binds 2 at most**, and 0 binds nothing.
    #[test]
    fn the_bound_version_is_two_at_most() {
        assert_eq!(ManagerVersion::bound(0), None);
        assert_eq!(ManagerVersion::bound(1), Some(ManagerVersion::V1));
        assert_eq!(ManagerVersion::bound(2), Some(ManagerVersion::V2));
        assert_eq!(ManagerVersion::bound(7), Some(ManagerVersion::V2));
    }

    /// **The window asks once**, at either version before its first commit,
    /// and after it only at version 2.
    #[test]
    fn the_ask_is_made_once_and_never_late_at_version_one() {
        for version in [ManagerVersion::V1, ManagerVersion::V2] {
            let mut n = Negotiation::new();
            assert!(n.ask(version, Toplevel::Uncommitted).is_some());
            assert!(
                n.ask(version, Toplevel::Uncommitted).is_none(),
                "{version:?}: twice"
            );
            assert!(
                n.ask(version, Toplevel::Committed).is_none(),
                "{version:?}: twice"
            );
        }
        let mut late = Negotiation::new();
        assert!(late.ask(ManagerVersion::V1, Toplevel::Committed).is_none());
        assert!(late.ask(ManagerVersion::V2, Toplevel::Committed).is_some());
    }

    /// **A configured mode is held until the ack**, the mode configured is
    /// the mode taken whichever it is, and a value naming no mode changes
    /// nothing.
    #[test]
    fn a_configure_is_taken_at_the_ack_and_an_invalid_one_is_not() {
        for (wire, want) in [
            (2, DecorationMode::ServerSide),
            (1, DecorationMode::ClientSide),
        ] {
            let mut n = Negotiation::new();
            let _ = n.ask(ManagerVersion::V2, Toplevel::Uncommitted);
            assert_eq!(n.configured(wire), Ok(want));
            assert_eq!(n.mode(), DecorationMode::ClientSide, "not before the ack");
            assert_eq!(n.acked(), want);
            assert_eq!(n.mode(), want);
        }
        let mut n = Negotiation::new();
        let _ = n.ask(ManagerVersion::V2, Toplevel::Uncommitted);
        let _ = n.configured(2);
        let _ = n.acked();
        assert_eq!(n.configured(3), Err(InvalidMode(3)));
        assert_eq!(
            n.acked(),
            DecorationMode::ServerSide,
            "an invalid mode is not taken"
        );
    }

    /// **At version 1 no buffer is attached between the ask and the first
    /// decoration configure**; at version 2, and without an ask, nothing
    /// holds it.
    #[test]
    fn version_one_holds_the_first_buffer_for_the_configure() {
        let mut v1 = Negotiation::new();
        assert!(v1.may_attach(), "nothing asked");
        let _ = v1.ask(ManagerVersion::V1, Toplevel::Uncommitted);
        assert!(!v1.may_attach());
        assert_eq!(v1.configured(4), Err(InvalidMode(4)));
        assert!(!v1.may_attach(), "an invalid configure is not one");
        let _ = v1.configured(1);
        assert!(v1.may_attach());
        let mut v2 = Negotiation::new();
        let _ = v2.ask(ManagerVersion::V2, Toplevel::Uncommitted);
        assert!(v2.may_attach());
    }
}
