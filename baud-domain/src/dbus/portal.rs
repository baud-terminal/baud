//! The desktop portal's Request protocol, which its print dialog answers
//! through (`[[source: xdg-portal-request]]`, `[[source: xdg-portal-print]]`).
//!
//! A portal call returns a Request object's path at once and answers later
//! with that object's `Response` signal. The path is predictable,
//! `/org/freedesktop/portal/desktop/request/SENDER/TOKEN`, "to let
//! applications subscribe to the Response signal before making the initial
//! portal call", and the caller "should verify that the returned handle is
//! what it expected, and update its signal subscription if it isn't". The
//! types below hold those rules: a token that is an object path element, a
//! path built from the caller's unique name, a returned handle compared with
//! it, a response code with the text's three meanings, and a `Response` read
//! only from a signal its sender vouched for.

use core::fmt;

use super::decode::{DictRead, Trusted};
use super::name::{BusKind, BusName, InterfaceName, MemberName, ObjectPath, Str};
use super::value::{Dict, Double, U32, Variant};
use crate::PaperMm;

/// The portal's well-known name.
pub const PORTAL: BusName<'static> = BusName::lit("org.freedesktop.portal.Desktop");
/// The portal's object.
pub const PORTAL_PATH: ObjectPath<'static> = ObjectPath::lit("/org/freedesktop/portal/desktop");
/// The print portal's interface.
pub const PRINT: InterfaceName<'static> = InterfaceName::lit("org.freedesktop.portal.Print");
/// The Request object's interface.
pub const REQUEST: InterfaceName<'static> = InterfaceName::lit("org.freedesktop.portal.Request");
/// The signal a Request answers with.
pub const RESPONSE: MemberName<'static> = MemberName::lit("Response");

/// Where every Request object's path begins.
const REQUEST_PREFIX: &str = "/org/freedesktop/portal/desktop/request/";

/// How many bytes of a [`HandleToken`] are in use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct TokenLen(u8);

/// A call's `handle_token`: "Must be a valid object path element"
/// (`[[source: xdg-portal-print]]`), so one or more of `[A-Za-z0-9_]`, and
/// here at most [`HandleToken::MAX`] of them. Built only by
/// [`HandleToken::parse`] and [`HandleToken::numbered`]:
///
/// ```compile_fail,E0451
/// use baud_domain::dbus::HandleToken;
/// let _ = HandleToken { text: [b'a'; 32], ..HandleToken::numbered(1) };
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::dbus::HandleToken;
/// assert_eq!(HandleToken::numbered(7).as_str(), "baud_7");
/// assert_eq!(HandleToken::parse("t1").map(|t| t.as_str() == "t1"), Some(true));
/// assert!(HandleToken::parse("a-b").is_none());
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HandleToken {
    text: [u8; HandleToken::MAX],
    len: TokenLen,
}

impl HandleToken {
    /// The longest token kept.
    pub const MAX: usize = 32;

    /// `s` as a token, or `None` when it is empty, longer than
    /// [`HandleToken::MAX`] or holds a byte outside `[A-Za-z0-9_]`.
    pub fn parse(s: &str) -> Option<HandleToken> {
        let b = s.as_bytes();
        let element = !b.is_empty()
            && b.len() <= HandleToken::MAX
            && b.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'_');
        if !element {
            return None;
        }
        let mut text = [0; HandleToken::MAX];
        text[..b.len()].copy_from_slice(b);
        Some(HandleToken {
            text,
            len: TokenLen(b.len() as u8),
        })
    }

    /// `baud_` and `n` in decimal: a fresh token for each call that counts
    /// its calls.
    pub fn numbered(n: u32) -> HandleToken {
        let mut text = [0; HandleToken::MAX];
        text[..5].copy_from_slice(b"baud_");
        let mut digits = [0u8; 10];
        let mut at = digits.len();
        let mut v = n;
        loop {
            at -= 1;
            digits[at] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        let tail = &digits[at..];
        text[5..5 + tail.len()].copy_from_slice(tail);
        HandleToken {
            text,
            len: TokenLen((5 + tail.len()) as u8),
        }
    }

    /// The token.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.text[..usize::from(self.len.0)]).unwrap_or_default()
    }
}

impl fmt::Debug for HandleToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("HandleToken").field(&self.as_str()).finish()
    }
}

/// How many bytes of a [`RequestPath`] are in use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct PathLen(u16);

/// The longest predicted path: the prefix, a unique name of at most 255
/// bytes less its `:`, a `/` and a token.
const PATH_MAX: usize = REQUEST_PREFIX.len() + 254 + 1 + HandleToken::MAX;

/// The Request path a call made with `token` will answer on: "SENDER is the
/// caller's unique name, with the initial ':' removed and all '.' replaced
/// by '_'" (`[[source: xdg-portal-request]]`). Built only by
/// [`RequestPath::predict`], from a unique name:
///
/// ```compile_fail,E0451
/// fn f(p: baud_domain::dbus::RequestPath) {
///     let _ = baud_domain::dbus::RequestPath { ..p };
/// }
/// ```
///
/// The legal path, the text's own form:
///
/// ```
/// use baud_domain::dbus::{BusName, HandleToken, RequestPath};
/// let token = HandleToken::parse("t1").expect("a token");
/// let path = RequestPath::predict(BusName::lit(":1.42"), &token).expect("a unique name");
/// assert_eq!(path.as_path().as_str(), "/org/freedesktop/portal/desktop/request/1_42/t1");
/// assert!(RequestPath::predict(BusName::lit("org.example.App"), &token).is_none());
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RequestPath {
    text: [u8; PATH_MAX],
    len: PathLen,
}

/// What a call's reply says of the Request path it was predicted to answer
/// on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle<'a> {
    /// The path predicted: the subscription made before the call stands.
    Expected,
    /// Another path, which the subscription must move to.
    Moved(ObjectPath<'a>),
}

impl RequestPath {
    /// The path a call from `unique` with `token` answers on, or `None`
    /// when `unique` is not a unique name or holds a `-`, which no object
    /// path element may.
    pub fn predict(unique: BusName<'_>, token: &HandleToken) -> Option<RequestPath> {
        if unique.kind() != BusKind::Unique {
            return None;
        }
        let sender = unique.as_str().as_bytes().get(1..)?;
        let mut text = [0; PATH_MAX];
        let mut at = 0;
        for &b in REQUEST_PREFIX.as_bytes() {
            text[at] = b;
            at += 1;
        }
        for &b in sender {
            text[at] = match b {
                b'.' => b'_',
                b'-' => return None,
                b => b,
            };
            at += 1;
        }
        text[at] = b'/';
        at += 1;
        for &b in token.as_str().as_bytes() {
            text[at] = b;
            at += 1;
        }
        let path = RequestPath {
            text,
            len: PathLen(at as u16),
        };
        ObjectPath::parse(path.as_str()).ok()?;
        Some(path)
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.text[..usize::from(self.len.0)]).unwrap_or_default()
    }

    /// The path. [`RequestPath::predict`] checked it against the object
    /// path grammar, so it parses, and the fallback is the portal's own
    /// object, never taken.
    pub fn as_path(&self) -> ObjectPath<'_> {
        ObjectPath::parse(self.as_str()).unwrap_or(PORTAL_PATH)
    }

    /// The call's reply, the Request path it returned, against this
    /// prediction.
    pub fn matches<'a>(&self, returned: ObjectPath<'a>) -> Handle<'a> {
        match returned.as_str() == self.as_str() {
            true => Handle::Expected,
            false => Handle::Moved(returned),
        }
    }
}

impl fmt::Debug for RequestPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RequestPath").field(&self.as_str()).finish()
    }
}

/// How the user's interaction ended (`[[source: xdg-portal-request]]`):
/// "0: Success, the request is carried out", "1: The user cancelled the
/// interaction", "2: The user interaction was ended in some other way". A
/// code the text does not give is kept as it came and never read as
/// success.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResponseCode {
    /// 0: the request is carried out.
    Success,
    /// 1: the user cancelled it.
    Cancelled,
    /// 2: it ended in some other way.
    Ended,
    /// Any other code, ended without success.
    Unknown(U32),
}

impl ResponseCode {
    /// The code `code` names.
    pub const fn of(code: U32) -> ResponseCode {
        match code.get() {
            0 => ResponseCode::Success,
            1 => ResponseCode::Cancelled,
            2 => ResponseCode::Ended,
            _ => ResponseCode::Unknown(code),
        }
    }
}

/// A Request's `Response(u response, a{sv} results)`, read only from a
/// signal its sender was vouched for ([`Trusted`]), so a `Response` another
/// connection emitted on the predicted path is never read as the portal's:
///
/// ```compile_fail,E0308
/// use baud_domain::dbus::{PortalResponse, Signal};
/// fn f(s: Signal<'_>) { let _ = PortalResponse::from_trusted(&s); }
/// ```
#[derive(Clone, Debug)]
pub struct PortalResponse<'a> {
    /// The Request object that answered.
    pub path: ObjectPath<'a>,
    /// How the interaction ended.
    pub code: ResponseCode,
    results: DictRead<'a, Str<'static>, Variant<'static>>,
}

impl<'a> PortalResponse<'a> {
    /// `signal` as a Request's `Response`, or `None` when it is another
    /// signal or its body is not `ua{sv}`.
    pub fn from_trusted(signal: &Trusted<'a>) -> Option<PortalResponse<'a>> {
        let s = signal.signal();
        if s.interface != REQUEST || s.member != RESPONSE {
            return None;
        }
        let (code, results) = s
            .body()
            .read::<(U32, Dict<'static, Str<'static>, Variant<'static>>)>()
            .ok()?;
        Some(PortalResponse {
            path: s.path,
            code: ResponseCode::of(code),
            results,
        })
    }

    /// The results, by key.
    pub fn results(&self) -> DictRead<'a, Str<'static>, Variant<'static>> {
        self.results.clone()
    }
}

/// A page setup's paper and margins (`[[source: xdg-portal-print]]`:
/// `Width`, `Height` and `Margin*` are `d`, "in millimeters"). A key that is
/// absent, of another type, not finite or below zero is `None`, and so is a
/// width or height of zero, which no paper has.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct PageSetup {
    /// `Width`: the paper's width.
    pub width: Option<PaperMm>,
    /// `Height`: the paper's height.
    pub height: Option<PaperMm>,
    /// `MarginTop`.
    pub top: Option<PaperMm>,
    /// `MarginBottom`.
    pub bottom: Option<PaperMm>,
    /// `MarginLeft`.
    pub left: Option<PaperMm>,
    /// `MarginRight`.
    pub right: Option<PaperMm>,
}

impl PageSetup {
    /// The keys of `page_setup` that are a page setup's.
    pub fn read(page_setup: DictRead<'_, Str<'static>, Variant<'static>>) -> PageSetup {
        let mut setup = PageSetup::default();
        for (key, value) in page_setup {
            let mm = value.read::<Double>().and_then(|d| PaperMm::of(d.get()));
            match key.as_str() {
                "Width" => setup.width = mm.filter(|m| m.get() > 0.0),
                "Height" => setup.height = mm.filter(|m| m.get() > 0.0),
                "MarginTop" => setup.top = mm,
                "MarginBottom" => setup.bottom = mm,
                "MarginLeft" => setup.left = mm,
                "MarginRight" => setup.right = mm,
                _ => {}
            }
        }
        setup
    }
}
