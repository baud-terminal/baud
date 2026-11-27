//! **A notification as a freedesktop notification server takes it**
//! (`[[source: freedesktop-notification-spec]]`), settled before any byte
//! is written: the text a program sent, made safe ([`SafeText`]); the body
//! escaped only for a server that listed `body-markup` ([`Escaped`]); the
//! icon a theme names ([`FdoIconName`]); and the arguments of one `Notify`
//! ([`FdoNotify`]). The key an action is sent and read back by is
//! [`FdoActionKey`], beside the slots a click is read against.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Deref;

use crate::{
    Activation, Expiry, FdoActionKey, MarkupListed, NonZeroMillis, NoticeButton, ServerCaps,
    Urgency,
};

/// **Text a program sent, made safe**: valid UTF-8 with no C0 or C1
/// control and no `DEL`. The protocol's "Escape code safe UTF-8" binds the
/// sender -- "it must contain none of the C0 and C1 control characters,
/// that is, the Unicode characters: U+0000 (NUL) - U+1F (Unit separator),
/// U+7F (DEL) and U+80 (PAD) - U+9F (APC)"
/// (`[[source: kitty-desktop-notifications]]`) -- and a desktop notification has no use
/// for a control either way, so what arrives is made so
/// ([`SafeText::clean`]): broken UTF-8 replaced, every control taken out.
///
/// ```
/// let t = baud_domain::SafeText::clean(b"a\x1b[31mb\xc2\x9bc\xff");
/// assert_eq!(t, "a[31mbc\u{fffd}");
/// let none = baud_domain::SafeText::clean(String::new().as_bytes());
/// assert!(none.is_empty());
/// ```
///
/// Text is made safe only by cleaning it:
///
/// ```compile_fail,E0308
/// let t = baud_domain::SafeText::from(String::new());
/// ```
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SafeText(String);

impl SafeText {
    /// **`bytes` made safe**: lossy UTF-8, every C0 and C1 control and
    /// `DEL` taken out.
    pub fn clean(bytes: &[u8]) -> SafeText {
        SafeText(
            String::from_utf8_lossy(bytes)
                .chars()
                .filter(|c| !c.is_control())
                .collect(),
        )
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text, moved out.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl Deref for SafeText {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for SafeText {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for SafeText {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl From<SafeText> for String {
    fn from(t: SafeText) -> String {
        t.0
    }
}

impl fmt::Display for SafeText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// **The parts of a text, `&`, `<` and `>` as entities where markup is
/// read**: with no proof, each text is one part as it is.
#[derive(Clone, Debug)]
struct Parts<'a> {
    rest: &'a str,
    markup: Option<MarkupListed>,
}

impl<'a> Iterator for Parts<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.rest.is_empty() {
            return None;
        }
        if self.markup.is_none() {
            return Some(core::mem::take(&mut self.rest));
        }
        let (part, rest) = match self.rest.find(['&', '<', '>']) {
            Some(0) => {
                let (c, rest) = self.rest.split_at(1);
                let entity = match c {
                    "&" => "&amp;",
                    "<" => "&lt;",
                    _ => "&gt;",
                };
                (entity, rest)
            }
            Some(at) => self.rest.split_at(at),
            None => (self.rest, ""),
        };
        self.rest = rest;
        Some(part)
    }
}

/// **A text escaped for a server that reads markup**: "Body text may
/// contain markup. The markup is XML-based"
/// (`[[source: freedesktop-notification-spec]]`), and what a program sent is text, so
/// `&`, `<` and `>` are written as entities. The parts are read in order
/// and nothing is copied. Escaping needs the server's word that it reads
/// markup ([`ServerCaps::markup`]):
///
/// ```
/// let t = baud_domain::SafeText::clean(b"a <b> & c");
/// let listed = baud_domain::ServerCaps::heard(["body-markup"]).markup().expect("listed");
/// let e: String = baud_domain::Escaped::of(&t, listed).collect();
/// assert_eq!(e, "a &lt;b&gt; &amp; c");
/// ```
///
/// and no proof is made for it:
///
/// ```compile_fail,E0423
/// let t = baud_domain::SafeText::clean(b"a <b> & c");
/// let e: String = baud_domain::Escaped::of(&t, baud_domain::MarkupListed(())).collect();
/// ```
#[derive(Clone, Debug)]
pub struct Escaped<'a>(Parts<'a>);

impl<'a> Escaped<'a> {
    /// `text`, escaped for a server that `listed` markup.
    pub fn of(text: &'a SafeText, listed: MarkupListed) -> Escaped<'a> {
        Escaped(Parts {
            rest: text.as_str(),
            markup: Some(listed),
        })
    }
}

impl<'a> Iterator for Escaped<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        self.0.next()
    }
}

/// **An icon name a theme resolves**: one of the protocol's "Universally
/// available icon names" (`[[source: kitty-desktop-notifications]]`) as
/// the freedesktop Icon Naming Specification names it
/// (`[[source: freedesktop-icon-naming-spec]]`), or any other name as sent, for the
/// theme to resolve. Never a path or a URI: `app_icon` takes "an URI
/// (file:// is the only URI schema supported right now)" too
/// (`[[source: freedesktop-notification-spec]]`), and a program must not make the
/// server open a file it names.
///
/// ```
/// let n = baud_domain::FdoIconName::of(&baud_domain::SafeText::clean(b"error"));
/// assert_eq!(n.as_ref().map(baud_domain::FdoIconName::as_str), Some("dialog-error"));
/// ```
///
/// A name is only what [`FdoIconName::of`] makes of what was sent:
///
/// ```compile_fail,E0277
/// let n: baud_domain::FdoIconName = "x".into();
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FdoIconName(IconFrom);

#[derive(Clone, PartialEq, Eq, Debug)]
enum IconFrom {
    Theme(&'static str),
    Sent(SafeText),
}

impl FdoIconName {
    /// The name the theme has for `sent`: `None` for none, and for a path
    /// or a URI.
    pub fn of(sent: &SafeText) -> Option<FdoIconName> {
        if sent.is_empty() || sent.contains(['/', ':']) {
            return None;
        }
        let theme = match sent.as_str() {
            "error" => "dialog-error",
            "warn" | "warning" => "dialog-warning",
            "info" => "dialog-information",
            "question" => "dialog-question",
            "help" => "help-browser",
            "file-manager" => "system-file-manager",
            "text-editor" => "accessories-text-editor",
            _ => return Some(FdoIconName(IconFrom::Sent(sent.clone()))),
        };
        Some(FdoIconName(IconFrom::Theme(theme)))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            IconFrom::Theme(n) => n,
            IconFrom::Sent(n) => n.as_str(),
        }
    }
}

/// `s`: "The default system sound for a notification", "No sound must
/// accompany the notification", or a sound by name
/// (`[[source: kitty-desktop-notifications]]`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NoticeSound {
    System,
    Silent,
    /// "error", "warn", "info", "question" or a platform's own name.
    Named(SafeText),
}

impl NoticeSound {
    /// The sound `name` asks for: none sent is the system's.
    pub fn of(name: SafeText) -> NoticeSound {
        match name.as_str() {
            "" | "system" => NoticeSound::System,
            "silent" => NoticeSound::Silent,
            _ => NoticeSound::Named(name),
        }
    }
}

/// `expire_timeout`: "If -1, the notification's expiration time is
/// dependent on the notification server's settings ... If 0, never
/// expire" (`[[source: freedesktop-notification-spec]]`); a timeout is at
/// least a millisecond.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdoExpire {
    Server,
    Never,
    After(NonZeroMillis),
}

impl FdoExpire {
    /// What a notification's `w` asks of the server.
    pub fn of(expiry: Expiry) -> FdoExpire {
        match expiry {
            Expiry::System => FdoExpire::Server,
            Expiry::Never => FdoExpire::Never,
            Expiry::After(ms) => FdoExpire::After(ms),
        }
    }
}

/// **A notification's parts that `Notify` carries**, as a driver holds
/// them, its text already safe: what [`FdoNotify::of`] reads.
#[derive(Clone, Copy, Debug)]
pub struct FdoPost<'a> {
    /// `f`, the program's application name.
    pub app: &'a SafeText,
    pub title: &'a SafeText,
    /// `OSC 1337 ; Notification`'s `subtitle`
    /// (`[[source: iterm2-964a75d-ptysession]]`).
    pub subtitle: &'a SafeText,
    pub body: &'a SafeText,
    /// The first `t`.
    pub kind: &'a SafeText,
    pub icon_names: &'a [SafeText],
    /// The buttons it shows, each with its number.
    pub buttons: &'a [(NoticeButton, SafeText)],
    pub sound: &'a NoticeSound,
    pub urgency: Urgency,
    pub expiry: Expiry,
    pub activation: Activation,
}

/// The label of the default action, for a server that shows it.
const OPEN: &[u8] = b"Open";

/// The application name sent when a program names none.
const BAUD: &[u8] = b"baud";

/// **One `Notify`'s arguments**
/// (`[[source: freedesktop-notification-spec]]`), made only by [`FdoNotify::of`]: the
/// program's application name or baud's; its first icon name as the theme
/// names it; the summary, and the body escaped where the server reads
/// markup; a `default` action where a click is reported or asked to focus
/// the window, and its buttons by number; the urgency, the sound, the type
/// as a category; and its expiry.
///
/// **`OSC 1337 ; Notification`'s subtitle**
/// (`[[source: iterm2-964a75d-ptysession]]`), which `Notify` has no
/// argument for, opens the body as its own paragraph, a blank line before
/// the message -- "Each line is a paragraph"; with no title it is the
/// summary, which is the one text argument the text does not call
/// optional.
///
/// ```
/// let text = |s: &[u8]| baud_domain::SafeText::clean(s);
/// let (empty, title, sub, body) = (text(b""), text(b"Build"), text(b"main"), text(b"a < b"));
/// let post = baud_domain::FdoPost {
///     app: &empty,
///     title: &title,
///     subtitle: &sub,
///     body: &body,
///     kind: &empty,
///     icon_names: &[],
///     buttons: &[],
///     sound: &baud_domain::NoticeSound::System,
///     urgency: baud_domain::Urgency::Normal,
///     expiry: baud_domain::Expiry::System,
///     activation: baud_domain::Activation::NONE,
/// };
/// let caps = baud_domain::ServerCaps::heard(["body-markup"]);
/// let n = baud_domain::FdoNotify::of(&post, caps);
/// assert_eq!((n.app_name().as_str(), n.summary().as_str()), ("baud", "Build"));
/// assert_eq!(n.body().collect::<String>(), "main\n\na &lt; b");
/// assert!(n.actions().is_empty());
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FdoNotify {
    app_name: SafeText,
    icon: Option<FdoIconName>,
    summary: SafeText,
    /// The subtitle, where it opens the body as its own paragraph.
    lead: SafeText,
    text: SafeText,
    markup: Option<MarkupListed>,
    actions: Vec<(FdoActionKey, SafeText)>,
    urgency: Urgency,
    sound: NoticeSound,
    category: Option<SafeText>,
    expire: FdoExpire,
}

impl FdoNotify {
    /// **`post` as `Notify` takes it, from a server that said it supports
    /// `caps`.**
    pub fn of(post: &FdoPost<'_>, caps: ServerCaps) -> FdoNotify {
        let (title, sub, body) = (post.title, post.subtitle, post.body);
        let (summary, lead, text) = match (title.is_empty(), sub.is_empty(), body.is_empty()) {
            (_, true, _) => (title, None, body),
            (true, false, _) => (sub, None, body),
            (false, false, true) => (title, None, sub),
            (false, false, false) => (title, Some(sub), body),
        };
        let mut actions = Vec::with_capacity(1 + post.buttons.len());
        if post.activation.reports() || post.activation.focuses() {
            actions.push((FdoActionKey::Default, SafeText::clean(OPEN)));
        }
        actions.extend(
            post.buttons
                .iter()
                .map(|(n, label)| (FdoActionKey::Button(*n), label.clone())),
        );
        FdoNotify {
            app_name: match post.app.is_empty() {
                true => SafeText::clean(BAUD),
                false => post.app.clone(),
            },
            icon: post.icon_names.first().and_then(FdoIconName::of),
            summary: summary.clone(),
            lead: lead.cloned().unwrap_or_else(|| SafeText(String::new())),
            text: text.clone(),
            markup: caps.markup(),
            actions,
            urgency: post.urgency,
            sound: post.sound.clone(),
            category: (!post.kind.is_empty()).then(|| post.kind.clone()),
            expire: FdoExpire::of(post.expiry),
        }
    }

    /// `app_name`.
    pub fn app_name(&self) -> &SafeText {
        &self.app_name
    }

    /// `app_icon`: "Can be an empty string, indicating no icon".
    pub fn icon(&self) -> Option<&FdoIconName> {
        self.icon.as_ref()
    }

    /// `summary`, never markup.
    pub fn summary(&self) -> &SafeText {
        &self.summary
    }

    /// `body`, in parts, escaped for a server that reads markup.
    pub fn body(&self) -> FdoBodyParts<'_> {
        let gap = match self.lead.is_empty() || self.text.is_empty() {
            true => "",
            false => "\n\n",
        };
        FdoBodyParts {
            now: Parts {
                rest: "",
                markup: self.markup,
            },
            queue: [self.lead.as_str(), gap, self.text.as_str()],
            next: 0,
        }
    }

    /// `actions`, each key with its label.
    pub fn actions(&self) -> &[(FdoActionKey, SafeText)] {
        &self.actions
    }

    /// The `urgency` hint's byte: "0 Low", "1 Normal", "2 Critical".
    pub fn urgency_level(&self) -> u8 {
        match self.urgency {
            Urgency::Low => 0,
            Urgency::Normal => 1,
            Urgency::Critical => 2,
        }
    }

    /// The sound it asks for.
    pub fn sound(&self) -> &NoticeSound {
        &self.sound
    }

    /// Its type, for the `category` hint.
    pub fn category(&self) -> Option<&SafeText> {
        self.category.as_ref()
    }

    /// `expire_timeout`.
    pub fn expire(&self) -> FdoExpire {
        self.expire
    }
}

/// **The parts of a [`FdoNotify`]'s body**, in order: the subtitle, the
/// blank line after it, the message, each escaped where the server reads
/// markup.
#[derive(Clone, Debug)]
pub struct FdoBodyParts<'a> {
    now: Parts<'a>,
    queue: [&'a str; 3],
    next: usize,
}

impl<'a> Iterator for FdoBodyParts<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        loop {
            if let Some(part) = self.now.next() {
                return Some(part);
            }
            let rest = *self.queue.get(self.next)?;
            self.next += 1;
            self.now.rest = rest;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn text(s: &str) -> SafeText {
        SafeText::clean(s.as_bytes())
    }

    /// Controls come out of the text a program sent, and broken UTF-8 is
    /// replaced: C0, `DEL` and C1, each taken out, the printable around
    /// them kept.
    #[test]
    fn safe_text_has_no_controls() {
        assert_eq!(
            SafeText::clean(b"a\x1b[31mb\xc2\x9bc\xff"),
            "a[31mbc\u{fffd}"
        );
        assert_eq!(
            SafeText::clean(b"\x00a\x1fb\x7fc\xc2\x80d\xc2\x9fe\xc2\xa0"),
            "abcde\u{a0}"
        );
        assert_eq!(text("x\ny\r\tz"), "xyz", "no newline either");
    }

    /// **Escaping only where markup is read**: every `&`, `<` and `>` an
    /// entity, at either end and side by side; nothing else touched.
    #[test]
    fn escaped_text_is_markup_that_reads_as_the_text() {
        let listed = ServerCaps::heard(["body-markup"]).markup().expect("listed");
        let esc = |s: &str| Escaped::of(&text(s), listed).collect::<String>();
        assert_eq!(esc("a <b> & c"), "a &lt;b&gt; &amp; c");
        assert_eq!(esc("<&>"), "&lt;&amp;&gt;");
        assert_eq!(esc("&amp;"), "&amp;amp;");
        assert_eq!(esc("plain \"'"), "plain \"'");
        assert_eq!(esc(""), "");
    }

    #[test]
    fn kittys_names_map_to_the_naming_specification() {
        let name = |s: &str| FdoIconName::of(&text(s)).map(|n| String::from(n.as_str()));
        assert_eq!(name("warn").as_deref(), Some("dialog-warning"));
        assert_eq!(name("error").as_deref(), Some("dialog-error"));
        assert_eq!(name("system-monitor").as_deref(), Some("system-monitor"));
        assert_eq!(name("org.example.App").as_deref(), Some("org.example.App"));
        assert_eq!(name("file:///etc/passwd"), None, "a URI is not a name");
        assert_eq!(name("/tmp/x.png"), None, "nor a path");
        assert_eq!(name(""), None, "no name, no icon");
    }

    /// A post with every part empty but what each case sets.
    struct Sent {
        app: SafeText,
        title: SafeText,
        subtitle: SafeText,
        body: SafeText,
        kind: SafeText,
        icons: Vec<SafeText>,
        buttons: Vec<(NoticeButton, SafeText)>,
        sound: NoticeSound,
        activation: Activation,
        expiry: Expiry,
    }

    impl Sent {
        fn new(title: &str, subtitle: &str, body: &str) -> Sent {
            Sent {
                app: SafeText::clean(b""),
                title: text(title),
                subtitle: text(subtitle),
                body: text(body),
                kind: SafeText::clean(b""),
                icons: Vec::new(),
                buttons: Vec::new(),
                sound: NoticeSound::System,
                activation: Activation::NONE,
                expiry: Expiry::System,
            }
        }

        fn notify(&self, caps: ServerCaps) -> FdoNotify {
            FdoNotify::of(
                &FdoPost {
                    app: &self.app,
                    title: &self.title,
                    subtitle: &self.subtitle,
                    body: &self.body,
                    kind: &self.kind,
                    icon_names: &self.icons,
                    buttons: &self.buttons,
                    sound: &self.sound,
                    urgency: Urgency::Critical,
                    expiry: self.expiry,
                    activation: self.activation,
                },
                caps,
            )
        }
    }

    /// **`OSC 1337 ; Notification`'s subtitle**: with a title, it opens
    /// the body as its own paragraph, or is the body when there is no
    /// message; with no title, it is the summary. The summary is never
    /// escaped; the body is, only for a server that reads markup.
    #[test]
    fn the_subtitle_is_the_bodys_first_paragraph_or_the_summary() {
        let markup = ServerCaps::heard(["body-markup"]);
        let plain = ServerCaps::heard([]);
        let cases = [
            (("T<", "", "B<"), ("T<", "B&lt;", "B<")),
            (("", "S<", "B<"), ("S<", "B&lt;", "B<")),
            (("T<", "S<", ""), ("T<", "S&lt;", "S<")),
            (("T<", "S<", "B<"), ("T<", "S&lt;\n\nB&lt;", "S<\n\nB<")),
            (("", "", "B"), ("", "B", "B")),
            (("T", "", ""), ("T", "", "")),
        ];
        for ((t, s, b), (summary, escaped, as_sent)) in cases {
            let sent = Sent::new(t, s, b);
            let n = sent.notify(markup);
            assert_eq!(n.summary(), &summary, "{t:?} {s:?} {b:?}");
            assert_eq!(n.body().collect::<String>(), escaped, "{t:?} {s:?} {b:?}");
            let n = sent.notify(plain);
            assert_eq!(n.body().collect::<String>(), as_sent, "{t:?} {s:?} {b:?}");
        }
    }

    /// **The rest of the arguments**: baud's name where the program sent
    /// none; the first icon name as the theme has it; `default` only where
    /// a click is reported or focuses, then each button by its number; the
    /// urgency's byte, the sound, the type and the expiry as asked.
    #[test]
    fn the_arguments_are_the_posts() {
        let mut sent = Sent::new("T", "", "");
        let n = sent.notify(ServerCaps::heard([]));
        assert_eq!(n.app_name(), &"baud");
        assert_eq!(n.icon(), None);
        assert!(n.actions().is_empty(), "nothing reported, nothing to click");
        assert_eq!(n.category(), None);
        assert_eq!(n.expire(), FdoExpire::Server);
        assert_eq!(n.urgency_level(), 2);

        let (one, three) = (
            NoticeButton::of_index(0).expect("one"),
            NoticeButton::of_index(2).expect("three"),
        );
        sent.app = text("org.example.App");
        sent.icons = vec![text("info"), text("error")];
        sent.buttons = vec![(one, text("Yes")), (three, text("No"))];
        sent.kind = text("im.received");
        sent.sound = NoticeSound::of(text("silent"));
        sent.activation = Activation::REPORT;
        sent.expiry = Expiry::After(NonZeroMillis::of::<1500>());
        let n = sent.notify(ServerCaps::heard([]));
        assert_eq!(n.app_name(), &"org.example.App");
        assert_eq!(
            n.icon().map(FdoIconName::as_str),
            Some("dialog-information")
        );
        let keys: Vec<(FdoActionKey, &str)> =
            n.actions().iter().map(|(k, l)| (*k, l.as_str())).collect();
        assert_eq!(
            keys,
            [
                (FdoActionKey::Default, "Open"),
                (FdoActionKey::Button(one), "Yes"),
                (FdoActionKey::Button(three), "No"),
            ]
        );
        assert_eq!(n.category(), Some(&text("im.received")));
        assert_eq!(n.sound(), &NoticeSound::Silent);
        assert_eq!(n.expire(), FdoExpire::After(NonZeroMillis::of::<1500>()));
        sent.activation = Activation::FOCUS;
        sent.expiry = Expiry::Never;
        let n = sent.notify(ServerCaps::heard([]));
        assert_eq!(
            n.actions()[0].0,
            FdoActionKey::Default,
            "a focus is a click"
        );
        assert_eq!(n.expire(), FdoExpire::Never);
    }

    /// `s`: none or `system` is the system's sound, `silent` none, any
    /// other a name.
    #[test]
    fn a_sound_is_the_systems_none_or_named() {
        assert_eq!(NoticeSound::of(text("")), NoticeSound::System);
        assert_eq!(NoticeSound::of(text("system")), NoticeSound::System);
        assert_eq!(NoticeSound::of(text("silent")), NoticeSound::Silent);
        assert_eq!(
            NoticeSound::of(text("warn")),
            NoticeSound::Named(text("warn"))
        );
    }
}
