//! **Text an effect carries to the desktop, owned and moved**: what goes
//! to the pasteboard ([`ClipText`]), what VoiceOver is given to say
//! ([`Spoken`]), and the identifier a notification is posted and removed
//! by ([`NoticeKey`]). Each is made once by the code that decided the
//! effect, from text it already owns, and moved into the effect queue's
//! turn (`os_effect.rs` in baud-app): never copied on the way, and never
//! mistaken for one another, which three `String`s would allow.

use alloc::string::String;

/// Text bound for the pasteboard: a selection, a link, a printed path, a
/// program's `OSC 52` write.
#[derive(Debug, PartialEq, Eq)]
pub struct ClipText(String);

/// Words for VoiceOver to say.
#[derive(Debug, PartialEq, Eq)]
pub struct Spoken(String);

/// The identifier a notification is posted under, and removed by.
#[derive(Debug, PartialEq, Eq)]
pub struct NoticeKey(String);

macro_rules! owned_text {
    ($t:ident) => {
        impl $t {
            /// The text, moved in.
            pub fn of(text: String) -> $t {
                $t(text)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// The text, moved out.
            pub fn into_string(self) -> String {
                self.0
            }
        }
    };
}

owned_text!(ClipText);
owned_text!(Spoken);
owned_text!(NoticeKey);

#[cfg(test)]
mod tests {
    use super::*;

    /// Moved in and out: none of the three is `Clone`, so the only way to
    /// a second copy is the caller's own, outside the effect.
    #[test]
    fn the_text_goes_in_and_comes_back_out() {
        let c = ClipText::of(String::from("copied"));
        assert_eq!(c.as_str(), "copied");
        assert_eq!(c.into_string(), "copied");
        let w = Spoken::of(String::from("said"));
        assert_eq!(w.into_string(), "said");
        assert_eq!(NoticeKey::of(String::from("n1")).as_str(), "n1");
    }
}
