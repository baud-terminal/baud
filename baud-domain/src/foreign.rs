//! **The window's export for another client to name it**,
//! `xdg-foreign-unstable-v2` (`[[source: xdg-foreign-unstable-v2]]`), as
//! types: when the export may be asked for, which handle it takes, and the
//! parent window a portal dialog is given from it. The presenter sends and
//! receives the messages; every rule about them is here.
//!
//! - "Only xdg_toplevel equivalent surfaces may be exported, otherwise an
//!   invalid_surface protocol error is sent." The window has one surface,
//!   its toplevel's, and the presenter exports that one.
//! - "A surface may be exported multiple times", and the window needs one
//!   handle, so [`Export::ask`] answers once with the [`ExportToplevel`]
//!   the presenter must hold to send `export_toplevel`.
//! - "The handle event will be sent immediately", carrying "a unique
//!   string". [`Export::handle`] takes it only after the ask and only
//!   once, and refuses an empty one, which would name no window
//!   ([`HandleRefused`]).
//! - "The exported surface may be referenced as long as the xdg_exported
//!   object not destroyed." The window never destroys it, so the handle
//!   it has is good for the window's life.
//! - The portal names a parent window "under Wayland" as
//!   `wayland:<HANDLE>`, and "if you don't have a suitable handle, just
//!   pass an empty string" ([`ParentWindow`], built only by
//!   [`Export::parent`]).

use alloc::string::String;

use crate::dbus::ParentWindow;

/// **The one ask the window may send**: `export_toplevel` on its
/// toplevel's surface. Only [`Export::ask`] makes one, so the request
/// cannot be sent twice.
///
/// ```compile_fail,E0451
/// let _ = baud_domain::ExportToplevel { _made_by_ask: () };
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct ExportToplevel {
    _made_by_ask: (),
}

/// **An exported surface handle**, as the compositor's `handle` event
/// gave it: never empty, and only [`Export::handle`] makes one, so a
/// parent window names only a handle the export gave.
///
/// ```compile_fail,E0451
/// let _ = baud_domain::SurfaceHandle { wire: String::from("h-1") };
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct SurfaceHandle {
    wire: String,
}

impl SurfaceHandle {
    /// The handle as the event carried it.
    pub fn as_str(&self) -> &str {
        &self.wire
    }
}

/// **Why a `handle` event was not taken.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleRefused {
    /// No export was asked for.
    NotAsked,
    /// The export has its handle already; it keeps that one.
    Again,
    /// The handle is empty, and would name no window.
    Empty,
}

/// Where the export is.
#[derive(Debug, PartialEq, Eq)]
enum State {
    Unasked,
    Asked,
    Handled(SurfaceHandle),
}

/// **The window's export**: whether it was asked for, and the handle it
/// has.
#[derive(Debug, PartialEq, Eq)]
pub struct Export {
    state: State,
}

impl Default for Export {
    fn default() -> Self {
        Export::new()
    }
}

impl Export {
    /// An export not asked for.
    pub const fn new() -> Export {
        Export {
            state: State::Unasked,
        }
    }

    /// **The ask, if the window may make it now**: once.
    pub fn ask(&mut self) -> Option<ExportToplevel> {
        match self.state {
            State::Unasked => {
                self.state = State::Asked;
                Some(ExportToplevel { _made_by_ask: () })
            }
            State::Asked | State::Handled(_) => None,
        }
    }

    /// **A `handle` event**: taken after the ask, once, and not empty.
    pub fn handle(&mut self, wire: &str) -> Result<&SurfaceHandle, HandleRefused> {
        match (&self.state, wire.is_empty()) {
            (State::Unasked, _) => return Err(HandleRefused::NotAsked),
            (State::Handled(_), _) => return Err(HandleRefused::Again),
            (State::Asked, true) => return Err(HandleRefused::Empty),
            (State::Asked, false) => {}
        }
        self.state = State::Handled(SurfaceHandle {
            wire: String::from(wire),
        });
        match &self.state {
            State::Handled(h) => Ok(h),
            State::Unasked | State::Asked => Err(HandleRefused::NotAsked),
        }
    }

    /// **The parent window a portal dialog is given**: the handle, once it
    /// has come, and unnamed until then.
    pub fn parent(&self) -> ParentWindow<'_> {
        match &self.state {
            State::Handled(h) => ParentWindow::Wayland(h),
            State::Unasked | State::Asked => ParentWindow::Unnamed,
        }
    }
}
