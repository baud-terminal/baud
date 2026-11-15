//! `OSC 52`, read a byte at a time.
//!
//! `[[source: ctlseqs]]` (2156-2180): "Ps = 5 2 -> Manipulate Selection Data
//! ... The parameter Pt is parsed as Pc ; Pd". `Pc` "may contain zero or more
//! characters from the set c , p , q , s , 0 , 1 , 2 , 3 , 4 , 5 , 6 , and 7
//! ... for clipboard, primary, secondary, select, or cut-buffers 0 through 7
//! respectively ... If the parameter is empty, xterm uses s 0". `Pd` "Normally
//! ... is a string encoded in base64 (RFC-4648)"; "If the second parameter is
//! a ? , xterm replies to the host with the selection data"; and "If the
//! second parameter is neither a base64 string nor ? , then the selection is
//! cleared."
//!
//! **Streamed, not parsed whole**, because `Pd` is unbounded and an OSC body
//! is otherwise kept in 512 bytes (`OSC_MAX` in the core). The core hands each
//! byte after `52;` to [`Osc52::push`] as it arrives, writes the octets it
//! releases wherever it keeps them, and asks [`Osc52::finish`] at the
//! terminator what the string was. The state is a few bytes and `Copy`, so it
//! lives in whatever outlives a read, and a body split anywhere is the same
//! sequence of calls as one that is not.

use crate::{Base64, Base64Step, Octets, ReadLen};

/// One of `Pc`'s selection parameters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    /// `c`, the clipboard.
    Clipboard,
    /// `p`, the primary selection.
    Primary,
    /// `q`, the secondary selection.
    Secondary,
    /// `s`, xterm's "configurable primary/clipboard selection".
    Select,
    /// `0` to `7`, the X cut buffers.
    CutBuffer(CutBuffer),
}

/// Which of the eight cut buffers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CutBuffer {
    B0,
    B1,
    B2,
    B3,
    B4,
    B5,
    B6,
    B7,
}

impl Selection {
    /// The selection a `Pc` character names, or `None` for a character
    /// outside ctlseqs' set.
    pub const fn of(b: u8) -> Option<Selection> {
        Some(match b {
            b'c' => Selection::Clipboard,
            b'p' => Selection::Primary,
            b'q' => Selection::Secondary,
            b's' => Selection::Select,
            b'0' => Selection::CutBuffer(CutBuffer::B0),
            b'1' => Selection::CutBuffer(CutBuffer::B1),
            b'2' => Selection::CutBuffer(CutBuffer::B2),
            b'3' => Selection::CutBuffer(CutBuffer::B3),
            b'4' => Selection::CutBuffer(CutBuffer::B4),
            b'5' => Selection::CutBuffer(CutBuffer::B5),
            b'6' => Selection::CutBuffer(CutBuffer::B6),
            b'7' => Selection::CutBuffer(CutBuffer::B7),
            _ => return None,
        })
    }

    const fn bit(self) -> u16 {
        1 << match self {
            Selection::Clipboard => 0,
            Selection::Primary => 1,
            Selection::Secondary => 2,
            Selection::Select => 3,
            Selection::CutBuffer(b) => {
                4 + match b {
                    CutBuffer::B0 => 0,
                    CutBuffer::B1 => 1,
                    CutBuffer::B2 => 2,
                    CutBuffer::B3 => 3,
                    CutBuffer::B4 => 4,
                    CutBuffer::B5 => 5,
                    CutBuffer::B6 => 6,
                    CutBuffer::B7 => 7,
                }
            }
        }
    }
}

/// The selections a `Pc` names, as a set.
///
/// **Order is not kept.** ctlseqs builds the list "in the order given", and
/// the order matters only to a read, which "uses the first selection found by
/// asking successively for each item" -- and a read is refused. A write sets
/// every selection named, and a set says that.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Selections(u16);

/// Set once any character of `Pc` arrived, named or not: what tells an
/// empty `Pc`, which means `s 0`, from one that named nothing ctlseqs knows.
const SPELLED: u16 = 1 << 12;

impl Selections {
    /// Nothing named, nothing spelled.
    pub const NONE: Selections = Selections(0);

    /// One more `Pc` character: its selection, if it names one.
    pub const fn push(self, b: u8) -> Selections {
        let named = match Selection::of(b) {
            Some(s) => s.bit(),
            None => 0,
        };
        Selections(self.0 | named | SPELLED)
    }

    /// `Pc` complete: an empty one is `s 0`, ctlseqs' "If the parameter is
    /// empty, xterm uses s 0".
    pub const fn resolve(self) -> Selections {
        match self.0 & SPELLED {
            0 => Selections(
                Selection::Select.bit() | Selection::CutBuffer(CutBuffer::B0).bit() | SPELLED,
            ),
            _ => self,
        }
    }

    /// Whether `s` is among them.
    pub const fn contains(self, s: Selection) -> bool {
        self.0 & s.bit() != 0
    }

    /// Whether any selection ctlseqs knows is named. A `Pc` of characters
    /// outside the set names none, and a write to it goes nowhere.
    pub const fn names_any(self) -> bool {
        self.0 & !SPELLED != 0
    }
}

/// Where in `Pc ; Pd` the string is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    /// Before the `;` that ends `Pc`.
    Targets,
    /// After it, with no `Pd` byte yet.
    Empty,
    /// `Pd` is exactly `?` so far.
    Question,
    /// `Pd` is data, and the decoder has every byte of it.
    Data,
}

/// An `OSC 52` body after its `52;`, a byte at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Osc52 {
    targets: Selections,
    part: Part,
    data: Base64,
}

/// What a complete `OSC 52` asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc52End {
    /// No `;` after `Pc`: there is no `Pd`, so the string is not ctlseqs'
    /// form at all.
    Malformed,
    /// `Pd` is `?`: a read of the first of these selections.
    Read(Selections),
    /// `Pd` is base64, and every octet it decoded to has been released --
    /// the last of them in the [`Octets`] [`Osc52::finish`] returns with
    /// this. An empty `Pd` is base64 of nothing, and is a write of nothing.
    Write(Selections),
    /// `Pd` is neither base64 nor `?`: "the selection is cleared". Octets the
    /// decoder released before it found that out are not the data.
    Clear(Selections),
}

impl Osc52 {
    /// The body's first byte after `52;` is next.
    pub const NEW: Osc52 = Osc52 {
        targets: Selections::NONE,
        part: Part::Targets,
        data: Base64::NEW,
    };

    /// One body byte: the octets of `Pd` a completed quantum released, as
    /// [`Base64::push`] answers. `Need` for every byte of `Pc`, and
    /// `Invalid` once `Pd` is known not to be base64.
    pub fn push(&mut self, b: u8) -> Base64Step {
        match self.part {
            Part::Targets => {
                if b == b';' {
                    self.part = Part::Empty;
                } else {
                    self.targets = self.targets.push(b);
                }
                Base64Step::Need
            }
            Part::Empty if b == b'?' => {
                self.part = Part::Question;
                Base64Step::Need
            }
            // A byte after a `?` makes `Pd` neither `?` nor base64, which the
            // decoder says when it is handed the `?` it was not given.
            Part::Question => {
                self.part = Part::Data;
                let _ = self.data.push(b'?');
                self.data.push(b)
            }
            Part::Empty | Part::Data => {
                self.part = Part::Data;
                self.data.push(b)
            }
        }
    }

    /// The terminator arrived: what the string asked for, and the octets of
    /// an unpadded final quantum to append to a write.
    pub fn finish(self) -> (Osc52End, Octets) {
        let targets = self.targets.resolve();
        let none = Octets::NONE;
        match self.part {
            Part::Targets => (Osc52End::Malformed, none),
            Part::Question => (Osc52End::Read(targets), none),
            Part::Empty => (Osc52End::Write(targets), none),
            Part::Data => match self.data.finish() {
                Ok(tail) => (Osc52End::Write(targets), tail),
                Err(_) => (Osc52End::Clear(targets), none),
            },
        }
    }
}

/// Whether the embedder puts a program's `OSC 52` write on the system's
/// clipboard: `On` until it says otherwise, and `Off` where its own
/// setting refuses them after the core has stored one (the app's
/// `[clipboard] osc52 = "off"`). Read by `OSC 61` alone, which names
/// `SetSelection` while it is off.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Osc52Writes {
    #[default]
    On,
    Off,
}

/// **Which control characters the embedder replaces in a paste**, which
/// `OSC 61 ; allowPasteControls` names: `Untold` until it says, and the
/// core claims nothing; `XtermDefault` for xterm's default
/// `disallowedPasteControls`, "BS,DEL,ENQ,EOT,ESC,NUL,STTY", each
/// replaced by a space while `allowPasteControls` is false, its default
/// (`[[source: xterm-man]]`; `removeControls`, `[[source: xterm-button]]`
/// 2582-2690), where every other C0 control passes unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PasteControls {
    #[default]
    Untold,
    XtermDefault,
}

/// xterm's default set: each name, and the byte it is when it is one;
/// `STTY` is the tty's special characters, read when a paste is sent.
const XTERM_DISALLOWED: [(&[u8], Option<u8>); 7] = [
    (b"BS", Some(0x08)),
    (b"DEL", Some(0x7f)),
    (b"ENQ", Some(0x05)),
    (b"EOT", Some(0x04)),
    (b"ESC", Some(0x1b)),
    (b"NUL", Some(0x00)),
    (b"STTY", None),
];

impl PasteControls {
    /// Whether the paste-control subcategory `name` is disallowed.
    pub fn disallows(self, name: &[u8]) -> bool {
        match self {
            PasteControls::Untold => false,
            PasteControls::XtermDefault => XTERM_DISALLOWED.iter().any(|(n, _)| *n == name),
        }
    }

    /// Whether a paste replaces `byte` whatever the tty says: one of the
    /// six named characters.
    pub fn replaces(self, byte: u8) -> bool {
        match self {
            PasteControls::Untold => false,
            PasteControls::XtermDefault => XTERM_DISALLOWED.iter().any(|(_, b)| *b == Some(byte)),
        }
    }

    /// Whether the tty's special characters are replaced too (`STTY`).
    pub fn stty(self) -> bool {
        self.disallows(b"STTY")
    }
}

/// How much clipboard memory a terminal is given: the largest copy it will
/// hold, in octets of text.
///
/// The core keeps two areas of that size -- the copy being collected and the
/// last one finished -- because the embedder reads a finished copy after the
/// read it ended in, and a second copy may begin in that same read. So the
/// buffer is twice the text, and a copy longer than the
/// text area is refused whole rather than cut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClipboardBudget {
    text: usize,
}

impl ClipboardBudget {
    /// Room for a copy of `text` octets.
    pub const fn of_text(text: usize) -> ClipboardBudget {
        ClipboardBudget { text }
    }

    /// Room for a copy whose base64 body is `body` bytes: three octets for
    /// every four characters, the ratio of section 4 of
    /// `[[source: rfc4648-base64]]`.
    pub const fn of_body(body: usize) -> ClipboardBudget {
        ClipboardBudget { text: body / 4 * 3 }
    }

    /// The largest copy, in octets.
    pub const fn text(self) -> usize {
        self.text
    }

    /// The bytes the two areas take together.
    pub const fn buffer(self) -> usize {
        self.text * 2
    }
}

/// What a finished `OSC 52` write leaves the selections holding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardContent<'a> {
    /// These octets: a base64 `Pd` decoded, empty for an empty `Pd`. Bytes,
    /// not text: whether they are UTF-8 is the embedder's to judge.
    Set(&'a [u8]),
    /// Nothing: `Pd` was neither base64 nor `?`, and ctlseqs says "the
    /// selection is cleared".
    Cleared,
}

/// The last `OSC 52` write the terminal honoured: which selections it named
/// and what they now hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClipboardWrite<'a> {
    pub targets: Selections,
    pub content: ClipboardContent<'a>,
}

/// Why an `OSC 52` write was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardRefusal {
    /// The storage was given no clipboard area.
    Unprovisioned,
    /// `Pc` named no selection ctlseqs knows.
    NoSelection,
    /// The decoded copy ran past the text area: its true length, which keeps
    /// counting after the area stops holding octets.
    Body(ReadLen),
}

/// The refused writes split by why, the [`ImageRefusals`] shape: their total
/// is the core's `clipboard_writes_refused`.
///
/// [`ImageRefusals`]: crate::ImageRefusals
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClipboardRefusals {
    /// Writes refused because the storage had no clipboard area.
    pub unprovisioned: usize,
    /// Writes refused because `Pc` named no selection.
    pub no_selection: usize,
    /// Writes refused because the copy ran past the text area.
    pub body: usize,
    /// The true length of the last copy refused for it, so a reader can see
    /// how far to raise the budget. Zero until one is.
    pub body_len: ReadLen,
}

impl ClipboardRefusals {
    /// Nothing refused.
    pub const NONE: ClipboardRefusals = ClipboardRefusals {
        unprovisioned: 0,
        no_selection: 0,
        body: 0,
        body_len: ReadLen::ZERO,
    };

    /// One more refusal.
    pub fn count(&mut self, why: ClipboardRefusal) {
        match why {
            ClipboardRefusal::Unprovisioned => self.unprovisioned += 1,
            ClipboardRefusal::NoSelection => self.no_selection += 1,
            ClipboardRefusal::Body(len) => {
                self.body += 1;
                self.body_len = len;
            }
        }
    }

    /// Every refusal, whatever its cause.
    pub fn total(&self) -> usize {
        self.unprovisioned + self.no_selection + self.body
    }
}
