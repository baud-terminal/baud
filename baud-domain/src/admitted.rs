//! **A URI the link policy admitted, owning its bytes**: what a link that
//! is opened or revealed carries from the driver that decided it to the
//! later turn that hands it to the OS, moved and never copied there.
//!
//! The policy is the app's (`baud-app/src/link.rs`); this type only keeps
//! what it admitted. Its one door, [`AdmittedUri::checked`], is called
//! from that module alone, after the checks, and a test there holds it to
//! that.

use alloc::string::String;

/// A URI the link policy admitted.
#[derive(Debug, PartialEq, Eq)]
pub struct AdmittedUri(String);

impl AdmittedUri {
    /// `uri`, which the link policy has just admitted.
    pub fn checked(uri: String) -> AdmittedUri {
        AdmittedUri(uri)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The URI's bytes, moved out.
    pub fn into_string(self) -> String {
        self.0
    }
}
