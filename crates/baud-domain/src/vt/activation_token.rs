//! **The token a notification server hands over for a click**: its
//! `ActivationToken` signal comes before `ActionInvoked`, and carries
//! "an activation token that can be used to activate a toplevel"
//! (`[[source: freedesktop-notification-spec]]`), which a
//! window passes to `xdg_activation_v1.activate`
//! (`[[source: xdg-activation-v1]]`).

use alloc::string::String;

/// **An activation token a notification server sent**: "either an X11-style
/// startup ID ... or a Wayland xdg-activation token"
/// (`[[source: freedesktop-notification-spec]]`), handed over with the one
/// click it came before. It is not `Clone`: the click that takes it passes
/// it on, and nothing keeps a copy. No clause makes a token single-use --
/// "Compositors can ignore unknown activation tokens when an invalid token
/// is passed" (`[[source: xdg-activation-v1]]`) -- so baud passes on the
/// text as it came and the compositor decides what it is worth.
///
/// ```
/// let t = baud_domain::XdgToken::heard("tok-42");
/// assert_eq!(t.as_str(), "tok-42");
/// let passed = t;
/// assert_eq!(passed.as_str(), "tok-42");
/// ```
///
/// A token is passed on, not copied:
///
/// ```compile_fail,E0599
/// let t = baud_domain::XdgToken::heard("tok-42");
/// let b = t.clone();
/// ```
#[derive(PartialEq, Eq, Debug)]
pub struct XdgToken(String);

impl XdgToken {
    /// The token an `ActivationToken` signal carried.
    pub fn heard(token: &str) -> XdgToken {
        XdgToken(String::from(token))
    }

    /// The token's text, as `xdg_activation_v1.activate` takes it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// **The token kept for a notification's click**: the last one heard,
/// with the id of the notification it came for, until a click on that
/// notification takes it or the notification closes. `I` is the server's
/// notification id.
///
/// ```
/// use baud_domain::{TokenFor, XdgToken};
/// let mut kept: TokenFor<u32> = TokenFor::new();
/// kept.keep(42, XdgToken::heard("tok-42"));
/// assert_eq!(kept.take_for(7), None);
/// assert_eq!(kept.take_for(42), Some(XdgToken::heard("tok-42")));
/// assert_eq!(kept.take_for(42), None);
/// ```
#[derive(PartialEq, Eq, Debug)]
pub enum TokenFor<I> {
    None,
    Kept(I, XdgToken),
}

impl<I: Copy + Eq> TokenFor<I> {
    /// None kept.
    pub const fn new() -> TokenFor<I> {
        TokenFor::None
    }

    /// Keep `token` for notification `id`, in place of any kept before.
    pub fn keep(&mut self, id: I, token: XdgToken) {
        *self = TokenFor::Kept(id, token);
    }

    /// The token kept for notification `id`, taken; one kept for another
    /// notification stays.
    pub fn take_for(&mut self, id: I) -> Option<XdgToken> {
        match core::mem::replace(self, TokenFor::None) {
            TokenFor::Kept(kept, token) if kept == id => Some(token),
            other => {
                *self = other;
                None
            }
        }
    }
}

impl<I: Copy + Eq> Default for TokenFor<I> {
    fn default() -> TokenFor<I> {
        TokenFor::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::{TokenFor, XdgToken};

    fn tok(s: &str) -> Option<XdgToken> {
        Some(XdgToken::heard(s))
    }

    /// **A token is handed over with the click on the notification it
    /// came for, once**: a click takes it, a token for another
    /// notification is not this one's and stays for its own, and the
    /// last token heard is the one kept.
    #[test]
    fn a_token_goes_only_to_its_own_notifications_click() {
        let mut kept: TokenFor<u32> = TokenFor::new();
        assert_eq!(kept.take_for(42), None, "none heard");
        kept.keep(42, XdgToken::heard("tok-42"));
        assert_eq!(kept.take_for(42), tok("tok-42"));
        assert_eq!(kept.take_for(42), None, "a click takes it");
        kept.keep(7, XdgToken::heard("tok-7"));
        assert_eq!(kept.take_for(42), None, "another notification's");
        assert_eq!(kept.take_for(7), tok("tok-7"), "kept for its own");
        kept.keep(7, XdgToken::heard("tok-7a"));
        kept.keep(8, XdgToken::heard("tok-8"));
        assert_eq!(kept.take_for(7), None, "the last heard is kept");
        assert_eq!(kept.take_for(8), tok("tok-8"));
        assert!(matches!(kept, TokenFor::None));
    }

    /// The token is the server's text as it came: the compositor decides
    /// what it is worth.
    #[test]
    fn a_token_is_the_text_the_server_sent() {
        assert_eq!(XdgToken::heard("a b/c").as_str(), "a b/c");
        assert_eq!(XdgToken::heard("").as_str(), "");
    }
}
