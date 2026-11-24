//! `OSC 50 ; Pt`, "Set Font to Pt", and its `?` query (`[[source: ctlseqs]]`),
//! out of the OSC dispatch's line: set and query, on by default, and a setting
//! turns changes off.
//!
//! **The core has no font.** The embedder tells it the family it draws in
//! and whether that is at the configured size (`Term::set_font`), and the
//! query is answered from that; a change is queued for the embedder
//! ([`FontAsks`]), which resolves the name against its faces and steps the
//! size, and tells the core what it drew in after. A change and a query in
//! one read answer the font from before the change: the queue is drained
//! after the read, where xterm changes the font before it parses the query.
//!
//! **What the index means**, xterm's font menu being one baud has not:
//! entry 0 is the configured size, `#+n` and `#-n` step the size `n`
//! points within 6-72, a step past them refused where the View menu's
//! Bigger and Smaller clamp, on Linux too, drawn at the pixels of the
//! scale the desktop asks for; entries 1-7 are none; a query of a relative
//! step names no numbered entry and gets xterm's empty reply.

use baud_domain::{
    ByteLen, FontChanges, FontEntry, FontIndex, FontRequest, FontSize, FontWritten, SizeStep,
    StringReply, StringTerm,
};

use crate::paint::Paint;

/// The longest family name the core keeps or queues. A name past it is
/// refused, as no face's family is that long: `name` strings are
/// "restricted to" 63 bytes for nameID 6 (`[[source: ms-opentype-name]]`),
/// and a family name is shorter in every face baud carries.
pub const FONT_NAME_MAX: usize = 64;

/// How many font changes the queue holds between two drains; one past it
/// is dropped and counted.
pub const FONT_QUEUE: usize = 4;

/// A change of font a program asked for: the size and, if `Pt` named one,
/// the family. The embedder performs it whole or not at all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FontChange {
    pub size: SizeStep,
    name: [u8; FONT_NAME_MAX],
    len: ByteLen,
    lead: Lead,
}

/// Which family a [`FontChange`] leads the chain with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontFamily<'a> {
    /// The family in force: a change of size alone.
    Kept,
    /// The family `Pt` named.
    Named(&'a [u8]),
    /// The family the embedder is configured with, which `RIS` asks back
    /// for after a program changed the font.
    Configured,
}

/// Whether a change leads with the family it names or the configured one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lead {
    AsNamed,
    Configured,
}

impl core::fmt::Debug for FontChange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FontChange")
            .field("size", &self.size)
            .field("name", &self.name().map(core::str::from_utf8))
            .field("lead", &self.lead)
            .finish()
    }
}

impl FontChange {
    /// The family named, or `None` for a change of size alone or one to
    /// the configured family.
    pub fn name(&self) -> Option<&[u8]> {
        match self.family() {
            FontFamily::Named(n) => Some(n),
            FontFamily::Kept | FontFamily::Configured => None,
        }
    }

    /// The family the change leads with.
    pub fn family(&self) -> FontFamily<'_> {
        match (self.lead, self.len.index()) {
            (Lead::Configured, _) => FontFamily::Configured,
            (Lead::AsNamed, 0) => FontFamily::Kept,
            (Lead::AsNamed, n) => FontFamily::Named(&self.name[..n]),
        }
    }
}

/// A count of the font requests that changed nothing -- refused while the
/// embedder had changes off, malformed, dropped from a full queue, or told
/// refused by the embedder (`Term::refuse_font_request`) -- as a reading
/// to compare.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontsRefused(u32);

/// The font the embedder draws in, the setting, the changes queued since
/// the last drain and the count of the requests that went nowhere.
#[derive(Clone, Copy)]
pub(crate) struct FontAsks {
    name: [u8; FONT_NAME_MAX],
    len: ByteLen,
    size: FontSize,
    pub(crate) changes: FontChanges,
    queue: [FontChange; FONT_QUEUE],
    queued: ByteLen,
    pub(crate) refused: FontsRefused,
    moved: FontMoved,
}

/// Whether a program has queued a change since power-on or the last
/// `RIS`, for `RIS` to ask for the configured font back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FontMoved {
    No,
    Yes,
}

/// The configured family at the configured size: what `RIS` asks for.
const CONFIGURED: FontChange = FontChange {
    size: SizeStep::Configured,
    name: [0; FONT_NAME_MAX],
    len: ByteLen::ZERO,
    lead: Lead::Configured,
};

impl FontAsks {
    pub(crate) const NONE: FontAsks = FontAsks {
        name: [0; FONT_NAME_MAX],
        len: ByteLen::ZERO,
        size: FontSize::Configured,
        changes: FontChanges::On,
        queue: [FontChange {
            size: SizeStep::Keep,
            name: [0; FONT_NAME_MAX],
            len: ByteLen::ZERO,
            lead: Lead::AsNamed,
        }; FONT_QUEUE],
        queued: ByteLen::ZERO,
        refused: FontsRefused(0),
        moved: FontMoved::No,
    };

    /// What the embedder draws in; a name past [`FONT_NAME_MAX`] is kept
    /// as none, so a query is not answered with a cut name.
    pub(crate) fn tell(&mut self, name: &[u8], size: FontSize) {
        let n = match name.len() <= FONT_NAME_MAX {
            true => name.len(),
            false => 0,
        };
        self.name[..n].copy_from_slice(&name[..n]);
        self.len = ByteLen::from_index(n);
        self.size = size;
    }

    /// The family the embedder last told, if it told one.
    pub(crate) fn known(&self) -> Option<&[u8]> {
        match self.len.index() {
            0 => None,
            n => Some(&self.name[..n]),
        }
    }

    pub(crate) fn queued(&self) -> &[FontChange] {
        &self.queue[..self.queued.index()]
    }

    pub(crate) fn clear(&mut self) {
        self.queued = ByteLen::ZERO;
    }

    /// **`RIS`**: after a program queued a change since power-on or the
    /// last `RIS`, the queue holds the configured font alone, which
    /// supersedes whatever it held; with none, nothing is asked.
    pub(crate) fn reset(&mut self) {
        if self.moved == FontMoved::Yes {
            self.queue[0] = CONFIGURED;
            self.queued = ByteLen::from_index(1);
            self.moved = FontMoved::No;
        }
    }

    pub(crate) fn refuse(&mut self) {
        self.refused = FontsRefused(self.refused.0.wrapping_add(1));
    }

    /// The entry an index expression names: entry 0, the current font at a
    /// size no entry holds, or none.
    fn entry(&self, i: FontIndex) -> Entry {
        match i.entry {
            FontEntry::Absolute(0) => Entry::Numbered,
            FontEntry::Absolute(_) => Entry::None,
            FontEntry::Relative(0) => match self.size {
                FontSize::Configured => Entry::Numbered,
                FontSize::Stepped => Entry::Unnumbered,
            },
            FontEntry::Relative(_) => Entry::None,
        }
    }
}

/// What a query's index names: entry 0, the current font at a size with no
/// entry, or nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Numbered,
    Unnumbered,
    None,
}

impl<M: crate::bands::BandMode> Paint<'_, '_, M> {
    /// Act on `OSC 50`.
    pub(crate) fn font_request(&mut self, req: FontRequest<'_>, term: StringTerm) {
        match req {
            FontRequest::Query(i) => self.font_query(i, term),
            FontRequest::Set { index, name } => self.font_set(index, name),
        }
    }

    /// **Answer `OSC 50 ; ?`** in xterm's shape (`QueryFontRequest`,
    /// `[[source: xterm-misc]]` 3798-3846): `OSC 50 ; name`, with `#0 `
    /// before the name when anything follows the `?` or the sign, as
    /// xterm's `*buf != 0` test gives, and `OSC 50`
    /// with no `;` when it names no entry. Silent with no font told, as an
    /// `OSC 10` query is with no colour.
    fn font_query(&mut self, i: FontIndex, term: StringTerm) {
        let asks = self.strs.fonts;
        let Some(name) = asks.known() else {
            return;
        };
        let frame = StringReply::osc(term);
        let body: [&[u8]; 2] = match (asks.entry(i), i.written) {
            (Entry::None, _) | (Entry::Unnumbered, FontWritten::Yes) => [b"50", b""],
            (_, FontWritten::No) => [b"50;", name],
            (Entry::Numbered, FontWritten::Yes) => [b"50;#0 ", name],
        };
        self.reply.lend(self.cold).string(frame, &body);
    }

    /// **Queue a change for the embedder**, or count it: refused while
    /// changes are off, or for an entry 1-7, a name too long to be a
    /// family, a request that changes nothing (`#` with no name at the
    /// current entry, or an empty `Pt`, which xterm rings its bell for), or
    /// a full queue.
    fn font_set(&mut self, index: Option<FontIndex>, name: &[u8]) {
        let asks = &mut self.strs.fonts;
        let size = match index.map(|i| i.entry) {
            None | Some(FontEntry::Relative(0)) => Some(SizeStep::Keep),
            Some(FontEntry::Absolute(0)) => Some(SizeStep::Configured),
            Some(FontEntry::Relative(n)) => Some(SizeStep::By(n)),
            Some(FontEntry::Absolute(_)) => None,
        };
        let at = asks.queued.index();
        let ok = asks.changes == FontChanges::On
            && name.len() <= FONT_NAME_MAX
            && at < FONT_QUEUE
            && !(name.is_empty() && size == Some(SizeStep::Keep));
        match (size, ok) {
            (Some(size), true) => {
                let mut c = FontChange {
                    size,
                    name: [0; FONT_NAME_MAX],
                    len: ByteLen::from_index(name.len()),
                    lead: Lead::AsNamed,
                };
                c.name[..name.len()].copy_from_slice(name);
                asks.queue[at] = c;
                asks.queued = ByteLen::from_index(at + 1);
                asks.moved = FontMoved::Yes;
            }
            _ => asks.refuse(),
        }
    }
}
