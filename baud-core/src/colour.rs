//! kitty's colour control, `OSC 21 ; key=value ; ... ST` (`[[source:
//! kitty-6d5d0c4-color-stack]]`): programs written for kitty read and set
//! the default colours, the selection's, the cursor's, the text under the
//! cursor and the 256 palette entries, in one sequence and one reply.
//!
//! **Routed by a prefix, as `52;` is** (`clip.rs`): once the body is `21;`,
//! each byte after it comes here, and each record is read at its `;`, so no
//! length limits a sequence -- kitty takes one up to 256 KiB
//! (`[[source: kitty-6d5d0c4-vt-parser]]` 18-21), and one setting the 256
//! entries is about 4 KB (Jev's lean, L_STREAM, 0.82/0.88 and 0.72/0.80
//! after research). A record is kept in `pending`; one longer than
//! `OSC_MAX` is refused and counted.
//!
//! **Nothing changes until the terminator.** A record's change is kept in
//! [`KittyColours`]'s shadow of the colours, which the records after it
//! read, so `foreground=white ; foreground=?` answers white, as kitty's
//! loop does (`[[source: kitty-window-py]]` 571-641); the terminator
//! writes the shadow through the setters, and a `CAN`, a `SUB` or an `ESC`
//! that is not `ST` reaches none, so a cancelled sequence changes nothing,
//! as ECMA-48 cancels a control string.
//!
//! **The reply is kitty's exactly**: one `OSC 21 ; key=value ; ... ESC \`
//! for the queries of a sequence, none when it asked nothing; each key as
//! it was sent and once, where it was first asked, with what the last ask
//! found, as kitty's responses dict keeps them; a colour as
//! `rgb:rr/gg/bb`; a colour with no defined value empty; a key baud does
//! not know `?`. Always `ESC \`, whatever ended the request, as kitty's
//! write_escape_code_to_child ends every OSC (`[[source:
//! kitty-6d5d0c4-screen]]` 1728-1741).

use crate::osc::StrKind;
use crate::paint::Paint;
use crate::storage::{
    CURSOR_TEXT, CURSOR_TEXT_CONFIGURED, EMBEDDER_PALETTE, OSC_MAX, REPLY_MOST, SELECTION_PER_CELL,
};
use baud_domain::themefile::CursorText;
use baud_domain::*;

/// The named colours a record can change: the default foreground and
/// background, the selection's two, the cursor's and the text under it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Named {
    Foreground,
    Background,
    SelectionBackground,
    SelectionForeground,
    Cursor,
    CursorText,
}

impl Named {
    const COUNT: usize = 6;

    const ALL: [Named; Named::COUNT] = [
        Named::Foreground,
        Named::Background,
        Named::SelectionBackground,
        Named::SelectionForeground,
        Named::Cursor,
        Named::CursorText,
    ];

    fn of(k: KittyKey) -> Option<Named> {
        Some(match k {
            KittyKey::Foreground => Named::Foreground,
            KittyKey::Background => Named::Background,
            KittyKey::SelectionBackground => Named::SelectionBackground,
            KittyKey::SelectionForeground => Named::SelectionForeground,
            KittyKey::Cursor => Named::Cursor,
            KittyKey::CursorText => Named::CursorText,
            KittyKey::VisualBell => return None,
        })
    }

    /// The `OSC 10`-family slot that keeps it; the text under the cursor
    /// has none and lives in the palette slice.
    fn dynamic(self) -> Option<DynamicColor> {
        Some(match self {
            Named::Foreground => DynamicColor::Foreground,
            Named::Background => DynamicColor::Background,
            Named::SelectionBackground => DynamicColor::HighlightBg,
            Named::SelectionForeground => DynamicColor::HighlightFg,
            Named::Cursor => DynamicColor::Cursor,
            Named::CursorText => return None,
        })
    }

    /// Whether an empty value means anything: kitty's table gives the
    /// default colours none ("Not applicable"), and its setter refuses one.
    fn takes_automatic(self) -> Automatic {
        match self {
            Named::Foreground | Named::Background => Automatic::NotApplicable,
            _ => Automatic::Taken,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Automatic {
    Taken,
    NotApplicable,
}

/// Whether the reply being built still fits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Room {
    Fits,
    /// An entry did not fit: the reply is dropped whole and counted, as a
    /// cut reply is a wrong one (`Paint::want_reply`).
    Overflowed,
}

/// Whether the record in `pending` arrived whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Record {
    Whole,
    /// Longer than `OSC_MAX`: the key or value read would not be the one
    /// sent, so it is refused.
    Cut,
}

/// What a query answers for one key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Answer {
    Colour(Chan, Chan, Chan),
    /// "If the queried color is one that does not have a defined value ...
    /// the value must be empty."
    Undefined,
    /// "If the terminal does not know a field ... it must respond back with
    /// the field=?".
    Unknown,
}

/// The longest `OSC 21` reply body: one control's answer less the seven
/// bytes of `ESC ] 2 1 ;` and `ESC \` round it.
const BODY_MAX: usize = REPLY_MOST - 7;

/// **The state of one `OSC 21` in flight**, beside the string storage:
/// the reply as the queries so far leave it, at most `BODY_MAX` bytes of body
/// (`Paint::colour_reply` says where its framing goes), and the shadow of
/// the colours its records changed. Not in `Cold`, whose size is pinned and which the
/// per-glyph write dereferences; behind a reference in `StrBuf`, for the
/// reason the kitty graphics stream is, and always provisioned, because it
/// is the protocol's state.
pub struct KittyColours {
    /// `key=value` entries joined by `;`, the reply's body after `21;`.
    reply: [u8; BODY_MAX],
    reply_len: ByteLen,
    room: Room,
    record: Record,
    /// A palette entry's override after the records so far: `None` where
    /// none changed it, `DEFAULT` for one reset.
    palette: [Option<ColorWord>; 256],
    named: [Option<ColorWord>; Named::COUNT],
}

impl KittyColours {
    /// Nothing in flight.
    pub const NEW: KittyColours = KittyColours {
        reply: [0; BODY_MAX],
        reply_len: ByteLen::ZERO,
        room: Room::Fits,
        record: Record::Whole,
        palette: [None; 256],
        named: [None; Named::COUNT],
    };

    /// A named colour's change after the records so far.
    fn named(&mut self, n: Named) -> &mut Option<ColorWord> {
        let [fg, bg, sel_bg, sel_fg, cursor, text] = &mut self.named;
        match n {
            Named::Foreground => fg,
            Named::Background => bg,
            Named::SelectionBackground => sel_bg,
            Named::SelectionForeground => sel_fg,
            Named::Cursor => cursor,
            Named::CursorText => text,
        }
    }

    /// The same, read.
    fn change(&self, n: Named) -> Option<ColorWord> {
        let [fg, bg, sel_bg, sel_fg, cursor, text] = self.named;
        match n {
            Named::Foreground => fg,
            Named::Background => bg,
            Named::SelectionBackground => sel_bg,
            Named::SelectionForeground => sel_fg,
            Named::Cursor => cursor,
            Named::CursorText => text,
        }
    }

    fn begin(&mut self) {
        self.reply_len = ByteLen::ZERO;
        self.room = Room::Fits;
        self.record = Record::Whole;
        self.palette = [None; 256];
        self.named = [None; Named::COUNT];
    }

    /// One query's answer, kept as kitty's dict keeps it: a key already
    /// asked keeps its place and takes the new value; a new one goes last.
    fn answer(&mut self, key: &[u8], a: Answer) {
        if self.room == Room::Overflowed {
            return;
        }
        let mut value = [0u8; 12];
        let v: &[u8] = match a {
            Answer::Colour(r, g, b) => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                value[..4].copy_from_slice(b"rgb:");
                for (i, c) in [r, g, b].into_iter().enumerate() {
                    let at = 4 + i * 3;
                    value[at] = HEX[usize::from(c.get() >> 4)];
                    value[at + 1] = HEX[usize::from(c.get() & 0xf)];
                    if i < 2 {
                        value[at + 2] = b'/';
                    }
                }
                &value[..12]
            }
            Answer::Undefined => b"",
            Answer::Unknown => b"?",
        };
        let len = self.reply_len.index();
        // The entry already there, if this key was asked before.
        let mut at = 0;
        while at < len {
            let end = self.reply[at..len]
                .iter()
                .position(|&b| b == b';')
                .map_or(len, |p| at + p);
            let entry = &self.reply[at..end];
            let eq = entry.iter().position(|&b| b == b'=').unwrap_or(entry.len());
            if &entry[..eq] == key {
                let (from, to) = (at + eq + 1, end);
                let grown = len - (to - from) + v.len();
                if grown > self.reply.len() {
                    self.room = Room::Overflowed;
                    return;
                }
                self.reply.copy_within(to..len, from + v.len());
                self.reply[from..from + v.len()].copy_from_slice(v);
                self.reply_len = ByteLen::from_index(grown);
                return;
            }
            at = end + 1;
        }
        let sep = usize::from(len > 0);
        let need = len + sep + key.len() + 1 + v.len();
        if need > self.reply.len() {
            self.room = Room::Overflowed;
            return;
        }
        let mut w = len;
        if sep == 1 {
            self.reply[w] = b';';
            w += 1;
        }
        self.reply[w..w + key.len()].copy_from_slice(key);
        w += key.len();
        self.reply[w] = b'=';
        w += 1;
        self.reply[w..w + v.len()].copy_from_slice(v);
        self.reply_len = ByteLen::from_index(need);
    }
}

impl Paint<'_, '_> {
    /// The body is `21;`: what follows is kitty's records.
    #[cold]
    #[inline(never)]
    pub(crate) fn colour_start(&mut self) {
        self.cold.str_kind = StrKind::ColorControl;
        self.cold.str_len = ByteLen::ZERO;
        self.strs.colours.begin();
    }

    /// One body byte past the `21;`. **Only printable ASCII is kept**, as
    /// kitty drops every other byte before it splits the body ("Only
    /// printable ASCII payload allowed as it is echoed back",
    /// `[[source: kitty-window-py]]` 575-576); a `;` ends a record.
    #[cold]
    #[inline(never)]
    pub(crate) fn colour_put(&mut self, b: StreamByte) {
        match b.raw_byte() {
            b';' => self.colour_record(),
            0x20..=0x7e => {
                let n = self.cold.str_len.index();
                if n < OSC_MAX {
                    self.strs.pending[n] = b.raw_byte();
                    self.cold.str_len = ByteLen::from_index(n + 1);
                } else {
                    self.strs.colours.record = Record::Cut;
                    self.string_truncations += 1;
                }
            }
            _ => {}
        }
    }

    /// The terminator: the last record, the shadow written through, and the
    /// reply.
    #[cold]
    #[inline(never)]
    pub(crate) fn colour_end(&mut self) {
        self.colour_record();
        self.colour_commit();
        self.colour_reply();
    }

    /// The reply the queries built, if any, and the state cleared.
    ///
    /// **The body is bounded by `BODY_MAX`**, seven bytes under one
    /// control's answer, `REPLY_MOST`, for the framing it goes out in,
    /// `ESC ] 2 1 ;` and `ESC \`, so the room a hold leaves always takes
    /// it. A longer body is dropped and counted by [`Room::Overflowed`], and
    /// one behind replies that leave it no room by `push_reply`. Either
    /// drops the reply whole, never a cut one.
    fn colour_reply(&mut self) {
        let c = &*self.strs.colours;
        let len = c.reply_len.index();
        match c.room {
            Room::Overflowed => self.reply_overflows += 1,
            Room::Fits if len > 0 => {
                let mut body = [0u8; BODY_MAX];
                body[..len].copy_from_slice(&c.reply[..len]);
                self.push_reply(&[b"\x1b]21;", &body[..len], b"\x1b\\"]);
            }
            Room::Fits => {}
        }
        self.strs.colours.begin();
    }

    /// An `OSC 21` whose body arrived whole ([`Osc::ColorControl`]), which
    /// in this core is `21` alone: each record in turn, as if streamed, the
    /// payload's buffer untouched (`dispatch_osc` has lent it out).
    pub(crate) fn colour_whole(&mut self, rest: &[u8]) {
        self.strs.colours.begin();
        for record in rest.split(|&b| b == b';') {
            let mut rec = [0u8; OSC_MAX];
            let mut n = 0;
            for &b in record.iter().filter(|b| matches!(b, 0x20..=0x7e)) {
                rec[n] = b;
                n += 1;
            }
            self.colour_apply(&rec[..n]);
        }
        self.colour_commit();
        self.colour_reply();
    }

    /// The record in `pending`.
    fn colour_record(&mut self) {
        let n = self.cold.str_len.index();
        self.cold.str_len = ByteLen::ZERO;
        if self.strs.colours.record == Record::Cut {
            self.strs.colours.record = Record::Whole;
            self.unsupported_total += 1;
            return;
        }
        let mut rec = [0u8; OSC_MAX];
        rec[..n].copy_from_slice(&self.strs.pending[..n]);
        self.colour_apply(&rec[..n]);
    }

    /// One record, applied to the shadow or answered.
    fn colour_apply(&mut self, rec: &[u8]) {
        let Some(r) = KittyRecord::parse(rec) else {
            return;
        };
        match (r.target, r.op) {
            (target, KittyOp::Query) => {
                let a = self.colour_answer(target);
                self.strs.colours.answer(r.key, a);
            }
            // A value the grammar does not read, or one a key cannot take:
            // a request not honoured, counted.
            (_, KittyOp::Unreadable) | (KittyTarget::Unknown, _) => self.unsupported_total += 1,
            // **The visual bell has no colour to set**: baud's flash is the
            // other video, which is the "Automatic color selection based on
            // current screen colors" kitty's table names, so an empty value
            // or a reset asks for what is drawn already, and a colour is
            // refused and counted (Jev's lean, V_EMPTY_COUNT, 0.38/0.76 and
            // 0.82/0.95 after research).
            (KittyTarget::Named(KittyKey::VisualBell), KittyOp::Set(..)) => {
                self.unsupported_total += 1
            }
            (KittyTarget::Named(KittyKey::VisualBell), _) => {}
            (KittyTarget::Named(k), op) => {
                let Some(named) = Named::of(k) else {
                    return;
                };
                let w = match op {
                    KittyOp::Set(r, g, b) => ColorWord::rgb(r, g, b),
                    KittyOp::Reset => ColorWord::DEFAULT,
                    _ => match named.takes_automatic() {
                        Automatic::Taken => ColorWord::AUTOMATIC,
                        // "Not applicable", and kitty's setter refuses it.
                        Automatic::NotApplicable => {
                            self.unsupported_total += 1;
                            return;
                        }
                    },
                };
                *self.strs.colours.named(named) = Some(w);
            }
            (KittyTarget::Entry(i), op) => {
                let w = match op {
                    KittyOp::Set(r, g, b) => ColorWord::rgb(r, g, b),
                    KittyOp::Reset => ColorWord::DEFAULT,
                    // **An empty value is refused and counted**: "Setting
                    // the ANSI color table colors to dynamic is not
                    // allowed" (`[[source: kitty-6d5d0c4-color-stack]]`
                    // 82), so the entry keeps what it had, as kitty's own
                    // code keeps a default colour asked to be dynamic
                    // (`[[source: kitty-window-py]]` 617-619, `[[source:
                    // kitty-6d5d0c4-colors-c]]` 825-840). Decided by Jev,
                    // REFUSE, 0.96/1.00 after research.
                    KittyOp::Automatic => {
                        self.unsupported_total += 1;
                        return;
                    }
                    // Taken by the arms above: answered, or counted.
                    KittyOp::Query | KittyOp::Unreadable => return,
                };
                self.strs.colours.palette[i.index()] = Some(w);
            }
        }
    }

    /// What a query finds, the records before it included. A named colour
    /// is the program's (an automatic one has no value), else the
    /// configured colour, else none. An entry is the program's, else the
    /// embedder's table, else none -- a core never told its colours has no
    /// value to give.
    fn colour_answer(&self, target: KittyTarget) -> Answer {
        let word = match target {
            KittyTarget::Unknown => return Answer::Unknown,
            KittyTarget::Named(KittyKey::VisualBell) => return Answer::Undefined,
            KittyTarget::Named(k) => {
                let Some(named) = Named::of(k) else {
                    return Answer::Undefined;
                };
                let own = self
                    .strs
                    .colours
                    .change(named)
                    .unwrap_or_else(|| self.named_now(named));
                match own {
                    w if w.is_automatic() => return Answer::Undefined,
                    w if !w.is_default() => w,
                    _ => self.named_configured(named),
                }
            }
            KittyTarget::Entry(i) => {
                let own =
                    self.strs.colours.palette[i.index()].unwrap_or(self.strs.palette[i.index()]);
                match own.is_default() {
                    true => self.strs.palette[EMBEDDER_PALETTE + i.index()],
                    false => own,
                }
            }
        };
        match word.as_rgb() {
            Some((r, g, b)) => Answer::Colour(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)),
            None => Answer::Undefined,
        }
    }

    /// The stored slot a named colour is under the video shown: **under
    /// `?5` foreground and background name the colour shown in that role**,
    /// as `OSC 10`/`11` do by the owner's V17 ("make OSC 4, 10 and 11 set,
    /// query and reset the colour you see"), which kitty's text replaces
    /// with these keys; so `background=?` and `OSC 11 ; ?` never disagree.
    /// Jev leaned to the stored colours, as kitty's code reads its profile
    /// (R_STORED, 0.40/0.34 after research, the first ask split): a lean
    /// under the bar, and the owner's call outranks it.
    fn slot(&self, n: Named) -> Option<DynamicColor> {
        n.dynamic().map(|d| self.cold.video().role(d))
    }

    /// A named colour's program override as stored.
    fn named_now(&self, n: Named) -> ColorWord {
        match self.slot(n) {
            Some(d) => self.cold.dynamic[d.index()],
            None => self.strs.palette[CURSOR_TEXT],
        }
    }

    /// A named colour as the embedder configured it, and none for a
    /// selection that takes each cell's own colours: it has no one colour,
    /// which "must be empty", though `OSC 17`/`19` answer the default text
    /// or page in its place ([`crate::ThemeColors::selection`]).
    fn named_configured(&self, n: Named) -> ColorWord {
        let per_cell = match n {
            Named::SelectionBackground => Some(SELECTION_PER_CELL),
            Named::SelectionForeground => Some(SELECTION_PER_CELL + 1),
            _ => None,
        };
        if per_cell.is_some_and(|i| self.strs.palette[i].is_automatic()) {
            return ColorWord::DEFAULT;
        }
        match self.slot(n) {
            Some(d) => self.cold.defaults[d.index()],
            // A kitty theme's `cursor none` sets its text aside while the
            // cursor shown has no colour: no defined value, so empty.
            None => match (
                self.cold.cursor_text_under,
                self.cold.shown(DynamicColor::Cursor).as_rgb(),
            ) {
                (CursorText::UnderACursorColour, None) => ColorWord::DEFAULT,
                _ => self.strs.palette[CURSOR_TEXT_CONFIGURED],
            },
        }
    }

    /// Every change the records made, through the setters, so the colour
    /// epoch moves only where a colour a rendition resolves through did.
    fn colour_commit(&mut self) {
        for i in 0..=u8::MAX {
            let c = Chan::from_u8(i);
            if let Some(w) = self.strs.colours.palette[c.index()] {
                self.set_palette(c, w);
            }
        }
        for n in Named::ALL {
            let Some(w) = self.strs.colours.change(n) else {
                continue;
            };
            match self.slot(n) {
                Some(d) => self.set_dynamic(d, w),
                None => self.strs.palette[CURSOR_TEXT] = w,
            }
        }
    }
}
