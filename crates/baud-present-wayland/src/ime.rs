//! **An input method through `text-input-v3`** (`[[source:
//! wp-text-input-unstable-v3]]`): the compositor's input method -- IBus,
//! Fcitx, a compositor's own -- shows what is being composed through
//! `preedit_string` and hands over the finished text through
//! `commit_string`, each applied on `done`; the terminal says where its
//! cursor is, so the candidate window opens beside it.
//!
//! The protocol's rules as this keeps them: text input is enabled on the
//! object's own `enter`, which "follows the keyboard focus", with the
//! content purpose `terminal` and the cursor rectangle, and one `commit`
//! applies them; every `commit` is counted, "the count as the serial in
//! done events". On `done` the pending preedit and commit replace the
//! current ones and reset to empty -- a `done` without a `preedit_string`
//! clears the preedit -- and changes are applied "as normal" whatever the
//! serial, but on a serial "different than the number of past commit
//! requests" no state is sent until a `done` matches. After `leave` "the
//! compositor must ignore requests", so a leave clears the preedit here
//! and sends nothing.
//!
//! **Version 2 where the compositor has it**, and every request it adds
//! only then, since a version-1 object has no such opcodes. The content
//! hint `preedit_shown`, because the preedit is drawn in place -- mutter
//! lets a version-2 client draw it only then
//! (`[[source: mutter-35f64c3d-text-input]]`) -- and its `preedit_hint`s
//! drawn as GTK's theme draws them (`baud_render`'s `Composition`), which
//! the text asks of a client that sets it. The one action, `submit`, is
//! offered and typed as the Return key, as the macOS app types AppKit's
//! `insertNewline:`. An on-screen keyboard is asked for with
//! `show_input_panel` when the primary button presses on the window while
//! text input is on, as GTK asks when a focused entry is clicked
//! (`[[source: gtk-3b438f05-imcontextwayland]]`), and `hide_input_panel`
//! is never sent, as GTK never sends it.
//!
//! Read and not acted on: `language`, which nothing in a terminal
//! reads, as GTK's handler is empty; and `delete_surrounding_text`,
//! which deletes from the surrounding text a terminal does not send --
//! "If the client is unaware of the text around the cursor, it should
//! not issue this request" -- as VTE (`[[source: vte-0-80-vte-cc]]`),
//! foot (`[[source: foot-2705e36-ime]]`) and kitty
//! (`[[source: kitty-aef2bc23-wl-text-input]]`) do not.
//!
//! The preedit's `cursor_begin` and `cursor_end` go with its text
//! ([`PreeditCursor`]), double-buffered as it is, for the app to draw the
//! cursor where the input method puts it.

use crate::shm::Px;
use baud_wayland::{Arg, ObjectId, Opcode, Sender, protocol};

/// The cell the cursor is in, in surface coordinates: what
/// `set_cursor_rectangle` carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorRect {
    pub x: Px,
    pub y: Px,
    pub width: Px,
    pub height: Px,
}

/// How many `commit` requests the object has sent: the serial a `done`
/// that knows the client's state carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Serial(pub u32);

/// Whether the last `done` knew every commit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sync {
    #[default]
    Current,
    /// A `done` named an older serial: hold state until one matches.
    Behind,
}

/// Whether text input is enabled on the window's surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Enabled {
    #[default]
    No,
    Yes,
}

/// One of `preedit_hint`'s styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    Whole,
    Selection,
    Prediction,
    Prefix,
    Suffix,
    SpellingError,
    ComposeError,
}

/// A value of the protocol's `preedit_hint` enum, as the wire carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HintCode(pub u32);

/// A value of the protocol's `action` enum, as the wire carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionCode(pub u32);

/// A byte offset into the preedit's text, as `preedit_hint` counts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteAt(pub u32);

impl Hint {
    /// The protocol's value, or `None` for one it does not name.
    fn of(code: HintCode) -> Option<Hint> {
        use protocol::zwp_text_input_v3::preedit_hint as h;
        Some(match code.0 {
            h::WHOLE => Hint::Whole,
            h::SELECTION => Hint::Selection,
            h::PREDICTION => Hint::Prediction,
            h::PREFIX => Hint::Prefix,
            h::SUFFIX => Hint::Suffix,
            h::SPELLING_ERROR => Hint::SpellingError,
            h::COMPOSE_ERROR => Hint::ComposeError,
            _ => return None,
        })
    }
}

/// A hint over the preedit's bytes `start..end`, as the event counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hinted {
    pub start: ByteAt,
    pub end: ByteAt,
    pub hint: Hint,
}

/// `preedit_string`'s `cursor_begin` or `cursor_end`: a byte offset into
/// the preedit's text, or -1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorAt(pub i32);

/// **Where the input method puts the cursor in its preedit**: "counted in
/// bytes relative to the beginning of the submitted text buffer", hidden
/// "when both are equal to -1", a line where the two are the same and a
/// highlight otherwise; both 0 until a `preedit_string` says, "the initial
/// value".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreeditCursor {
    pub begin: CursorAt,
    pub end: CursorAt,
}

/// What the input method is composing, the hints it draws it with and
/// its cursor in it; empty for none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    pub hints: Vec<Hinted>,
    pub cursor: PreeditCursor,
}

impl From<&str> for Preedit {
    /// The text with no hints and the cursor after it.
    fn from(text: &str) -> Preedit {
        let at = CursorAt(i32::try_from(text.len()).unwrap_or(i32::MAX));
        Preedit {
            text: text.to_string(),
            hints: Vec::new(),
            cursor: PreeditCursor { begin: at, end: at },
        }
    }
}

/// What a `done` applied, in the protocol's order, for the window to
/// report.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// The preedit is now this; empty for none.
    Preedit(Preedit),
    /// This is typed.
    Commit(String),
    /// "Perform the requested action": the only one offered, `submit`.
    Submit,
}

/// The action a `done` performs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Action {
    #[default]
    None,
    Submit,
}

/// Whether an on-screen keyboard has been asked for and not yet sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Panel {
    #[default]
    Quiet,
    Owed,
}

/// The text-input object and its double-buffered state.
#[derive(Debug, Default)]
pub struct TextInput {
    pub manager: Option<ObjectId>,
    /// The version the manager was bound at, which its objects have.
    pub version: u32,
    object: Option<ObjectId>,
    enabled: Enabled,
    commits: Serial,
    sync: Sync,
    pending_text: String,
    pending_hints: Vec<Hinted>,
    pending_cursor: PreeditCursor,
    pending_commit: String,
    pending_action: Action,
    preedit: Preedit,
    panel: Panel,
    /// The rectangle the window last asked for, and the one last sent.
    rect: Option<CursorRect>,
    sent: Option<CursorRect>,
}

/// The first version with actions, the input panel, the preedit hints
/// and the content hints after `multiline`.
const VERSION_2: u32 = 2;

impl TextInput {
    /// `get_text_input`, once both the manager and the seat exist.
    pub fn object_for(&mut self, s: &mut Sender<'_>, seat: Option<ObjectId>) {
        let (Some(manager), Some(seat), None) = (self.manager, seat, self.object) else {
            return;
        };
        let id = s.new_id(&protocol::zwp_text_input_v3::INTERFACE);
        let _ = s.request(
            manager,
            Opcode(protocol::zwp_text_input_manager_v3::request::GET_TEXT_INPUT),
            &[Arg::NewId(id), Arg::Object(seat)],
        );
        self.object = Some(id);
    }

    pub fn is(&self, id: ObjectId) -> bool {
        self.object == Some(id)
    }

    fn request(&self, s: &mut Sender<'_>, op: Opcode, args: &[Arg<'_>]) {
        if let Some(o) = self.object {
            let _ = s.request(o, op, args);
        }
    }

    fn commit(&mut self, s: &mut Sender<'_>) {
        self.request(s, Opcode(protocol::zwp_text_input_v3::request::COMMIT), &[]);
        self.commits = Serial(self.commits.0.wrapping_add(1));
    }

    fn send_rect(&mut self, s: &mut Sender<'_>, r: CursorRect) {
        let int = |p: Px| Arg::Int(i32::try_from(p.0).unwrap_or(i32::MAX));
        self.request(
            s,
            Opcode(protocol::zwp_text_input_v3::request::SET_CURSOR_RECTANGLE),
            &[int(r.x), int(r.y), int(r.width), int(r.height)],
        );
        self.sent = Some(r);
    }

    /// `enter` on the window's surface: enable, a terminal's content
    /// type, at version 2 the `submit` action, the cursor where it is
    /// known, one commit. "After an enter event ... all state information
    /// is invalidated and needs to be resent by the client."
    pub fn entered(&mut self, s: &mut Sender<'_>) {
        use protocol::zwp_text_input_v3::{action, content_hint, content_purpose, request};
        self.enabled = Enabled::Yes;
        self.sync = Sync::Current;
        self.sent = None;
        self.request(s, Opcode(request::ENABLE), &[]);
        let hint = match self.version >= VERSION_2 {
            true => content_hint::PREEDIT_SHOWN,
            false => content_hint::NONE,
        };
        self.request(
            s,
            Opcode(request::SET_CONTENT_TYPE),
            &[Arg::Uint(hint), Arg::Uint(content_purpose::TERMINAL)],
        );
        if self.version >= VERSION_2 {
            let submit = action::SUBMIT.to_ne_bytes();
            self.request(
                s,
                Opcode(request::SET_AVAILABLE_ACTIONS),
                &[Arg::Array(&submit)],
            );
        }
        if let Some(r) = self.rect {
            self.send_rect(s, r);
        }
        self.commit(s);
    }

    /// **The pointer pressed on the window**: an on-screen keyboard asked
    /// for, at version 2 and while text input is on, sent with a commit
    /// once the client is current.
    pub fn pressed(&mut self, s: &mut Sender<'_>) {
        if self.version >= VERSION_2 && self.enabled == Enabled::Yes {
            self.panel = Panel::Owed;
            self.flush(s);
        }
    }

    /// `leave`: nothing is sent, and the preedit is gone. Its clearing,
    /// if there was one to clear.
    pub fn left(&mut self) -> Option<Preedit> {
        self.enabled = Enabled::No;
        self.panel = Panel::Quiet;
        self.pending_text.clear();
        self.pending_hints.clear();
        self.pending_commit.clear();
        self.pending_action = Action::None;
        match self.preedit.text.is_empty() {
            true => None,
            false => {
                self.preedit = Preedit::default();
                Some(Preedit::default())
            }
        }
    }

    /// The pending preedit's text and its cursor.
    pub fn preedit_string(&mut self, text: Option<&str>, cursor: PreeditCursor) {
        self.pending_text = text.unwrap_or_default().to_string();
        self.pending_cursor = cursor;
    }

    /// `preedit_hint`: kept for the pending preedit, a value the protocol
    /// does not name dropped. A hint "is always sent together with a
    /// zwp_text_input_v3.preedit_string event", in no order the text
    /// fixes, and both are "applied and reset on the next
    /// zwp_text_input_v3.done event", so the two are kept apart until it.
    pub fn preedit_hint(&mut self, start: ByteAt, end: ByteAt, hint: HintCode) {
        if let Some(hint) = Hint::of(hint) {
            self.pending_hints.push(Hinted { start, end, hint });
        }
    }

    /// `action`: `submit`, the one offered, performed on the next `done`;
    /// `none` or a value the protocol does not name is nothing to do.
    pub fn action(&mut self, code: ActionCode) {
        self.pending_action = match code.0 {
            protocol::zwp_text_input_v3::action::SUBMIT => Action::Submit,
            _ => Action::None,
        };
    }

    pub fn commit_string(&mut self, text: Option<&str>) {
        self.pending_commit = text.unwrap_or_default().to_string();
    }

    /// `done`: "1. Replace existing preedit string with the cursor ... 3.
    /// Insert commit string ... 5. Insert new preedit text ... 7. Perform
    /// the requested action", each step only where it changes something,
    /// and the cursor sent if the serial is the client's and it moved
    /// meanwhile.
    pub fn done(&mut self, s: &mut Sender<'_>, serial: Serial) -> Vec<Step> {
        let commit = std::mem::take(&mut self.pending_commit);
        let preedit = Preedit {
            text: std::mem::take(&mut self.pending_text),
            hints: std::mem::take(&mut self.pending_hints),
            cursor: std::mem::take(&mut self.pending_cursor),
        };
        let action = std::mem::take(&mut self.pending_action);
        let mut steps = Vec::new();
        if commit.is_empty() {
            if preedit != self.preedit {
                steps.push(Step::Preedit(preedit.clone()));
            }
        } else {
            if !self.preedit.text.is_empty() {
                steps.push(Step::Preedit(Preedit::default()));
            }
            steps.push(Step::Commit(commit));
            if !preedit.text.is_empty() {
                steps.push(Step::Preedit(preedit.clone()));
            }
        }
        if action == Action::Submit {
            steps.push(Step::Submit);
        }
        self.preedit = preedit;
        self.sync = match serial == self.commits {
            true => Sync::Current,
            false => Sync::Behind,
        };
        self.flush(s);
        steps
    }

    /// **Where the cursor is**, sent with a commit when text input is on,
    /// the client is current and it moved; held otherwise, and sent when
    /// it can be.
    pub fn set_cursor(&mut self, s: &mut Sender<'_>, r: CursorRect) {
        self.rect = Some(r);
        self.flush(s);
    }

    fn flush(&mut self, s: &mut Sender<'_>) {
        if self.enabled == Enabled::No || self.sync == Sync::Behind {
            return;
        }
        let rect = self.rect.filter(|r| Some(*r) != self.sent);
        if rect.is_none() && self.panel == Panel::Quiet {
            return;
        }
        if let Some(r) = rect {
            self.send_rect(s, r);
        }
        if self.panel == Panel::Owed {
            self.panel = Panel::Quiet;
            self.request(
                s,
                Opcode(protocol::zwp_text_input_v3::request::SHOW_INPUT_PANEL),
                &[],
            );
        }
        self.commit(s);
    }
}
