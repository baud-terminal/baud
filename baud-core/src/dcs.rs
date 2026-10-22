//! DECRQSS, and XTGETTCAP and XTSETTCAP below it; Sixel (`sixel.rs`,
//! entered by `Act::SixelStart`), ReGIS and DECRSPS are elsewhere.
//!
//! `[[source: ctlseqs]]` gives `DCS $ q Pt ST` as Request Status String, where
//! `Pt` names a setting by the tail of the CSI that would set it -- `m` for
//! SGR, `r` for DECSTBM, `"q"` for DECSCA. The reply is the *tail of the CSI
//! that would set it*, so an application can echo it straight back.
//!
//! Every other DCS -- DECUDK, DECAUPSS (`DCS Ps ! u ... ST`, the one that
//! assigns the user-preference supplemental set; DECRQUPSS, its query, is
//! `CSI & u` and not a DCS) -- is consumed to its terminator and ignored, which
//! is the half that matters: a DCS body reaching the screen would render an
//! application's private protocol as glyphs.

use crate::osc::StrKind;
use crate::paint::Paint;
use crate::storage::{OSC_MAX, REPLY};
use crate::write::Csi;
use baud_domain::*;

/// Whether an attribute's `WIRE` code is a sub-parameter, written `4:n`.
///
/// The three colon underline styles have no one-parameter spelling, so the
/// answer round-trips through `SgrAttr::parse`'s colon arm only in this form.
/// One predicate for the encoder and for [`DECRQSS_MAX`], so the bound cannot
/// count a spelling the encoder does not write.
const fn colon_style(bit: Attrs) -> bool {
    matches!(
        bit,
        Attrs::CURLY_UNDERLINE | Attrs::DOTTED_UNDERLINE | Attrs::DASHED_UNDERLINE
    )
}

/// The longest DECRQSS reply this encoder can write: **126 bytes**.
///
/// Built from the encoder's own parts rather than measured off one rendition,
/// so a new attribute in `Attrs::WIRE` grows it without anyone remembering to.
/// The SGR body is the longest of the five answers, and in it:
///
/// * every `WIRE` bit, each as `;` and its code at its own width, with `4:`
///   ahead of the three colon styles -- 63 bytes. **An upper bound, not a
///   reachable reply**: `4`, `21` and every `4:n` replace the underline
///   family, so a rendition holds at most one of `4`, `21`, `4:3`, `4:4` and
///   `4:5`, and thirteen of those bytes are never sent together; `5` and `6`
///   replace the blink family, so two more are not;
/// * a font, `;19` at most, because `FontId::sgr` is `10..=19`;
/// * three colours at their widest, `;58;2;255;255;255`, which is longer than
///   any indexed `;58;5;255`;
/// * the leading `0` and the final `m`.
///
/// Around it, the seven-bit frame, the longer of the two C1 forms: `ESC P`,
/// `1 $ r` and `ESC \`. A margin pair (`65535;65535r`, the widest `OneBased`),
/// a cursor style and a protection flag are each far shorter.
///
/// The richest rendition an application can set answers in 111 bytes, and
/// `the_longest_decrqss_reply_is_within_its_bound` drives it through the
/// encoder against this. Note AG, section 1.2, measured 114 while `21` could
/// be held beside `4:3`; the family is exclusive now and the two cannot meet.
pub const DECRQSS_MAX: usize = {
    let mut attrs = 0;
    let mut i = 0;
    while i < Attrs::WIRE.len() {
        let (bit, code) = Attrs::WIRE[i];
        let digits = if code >= 100 {
            3
        } else if code >= 10 {
            2
        } else {
            1
        };
        attrs += digits
            + match colon_style(bit) {
                true => b";4:".len(),
                false => b";".len(),
            };
        i += 1;
    }
    let sgr = b"0".len() + attrs + b";19".len() + 3 * b";58;2;255;255;255".len() + b"m".len();
    let others = [
        b"65535;65535r".len(),
        b"65535;65535s".len(),
        b"255 q".len(),
        b"255\"q".len(),
        b"132$|".len(),
        b"65535t".len(),
        b">4;2m".len(),
        b">0;2t".len(),
    ];
    let mut body = sgr;
    let mut k = 0;
    while k < others.len() {
        if others[k] > body {
            body = others[k];
        }
        k += 1;
    }
    b"\x1bP".len() + b"1$r".len() + body + b"\x1b\\".len()
};

// **A drained buffer always takes a DECRQSS answer.** `REPLY`'s doc names the
// rendition string as the longest single thing it holds; this is that claim
// as a build failure rather than a sentence.
const _: () = assert!(DECRQSS_MAX <= REPLY);

/// The longest XTGETTCAP answer: a list of `OSC_MAX` bytes less its `q`,
/// every name the shortest and every value the longest one name answers
/// (`tcap_reply_max`). **The reply buffer holds it**, so a request alone is
/// never dropped; behind other replies it is sent whole or counted.
pub const XTGETTCAP_MAX: usize = tcap_reply_max(OSC_MAX - 1);
const _: () = assert!(XTGETTCAP_MAX <= REPLY);

/// What a `DCS +` reads and leaves: the name `TN` answers, the embedder's
/// (`Term::set_term_name`), which `RIS` keeps; and the XTSETTCAPs refused.
pub(crate) struct Tcap {
    pub(crate) name: TermName,
    pub(crate) sets_refused: TcapSetsRefused,
}

impl Tcap {
    pub(crate) const UNTOLD: Tcap = Tcap {
        name: TermName::UNTOLD,
        sets_refused: TcapSetsRefused::NONE,
    };
}

/// How many XTSETTCAPs were refused since power-on, as a reading to
/// compare (`Term::tcap_sets_refused`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TcapSetsRefused(u32);

impl TcapSetsRefused {
    /// None yet.
    pub const NONE: TcapSetsRefused = TcapSetsRefused(0);

    fn next(self) -> TcapSetsRefused {
        TcapSetsRefused(self.0.wrapping_add(1))
    }
}

/// Whether an XTGETTCAP list reached its terminator whole.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TcapBody {
    Whole,
    /// Cut at `OSC_MAX`, and the cut counted in `string_truncations`.
    Cut,
}

/// A `DCS +`: XTGETTCAP or XTSETTCAP, which its body's first byte names
/// at the terminator. **A parameter before the `+` makes it neither**:
/// xterm's `do_dcs` switches on the string's first byte, so `DCS 1 + q` is
/// not its `+` case (`[[source: xterm-misc]]`'s file, 5160-5170), and it is
/// consumed and counted as any DCS no handler reads.
pub(crate) fn tcap_start(csi: &Csi) -> StrKind {
    match csi.untouched() {
        true => StrKind::Tcap,
        false => StrKind::Other,
    }
}

impl Paint<'_, '_> {
    /// A `DCS +` string's terminator: `q` is XTGETTCAP and `p` XTSETTCAP
    /// (`[[source: ctlseqs]]` 535-582); any other byte is a DCS no handler
    /// reads, counted as one.
    ///
    /// **XTGETTCAP is answered** by `encode_tcap_reply`, from xterm's table
    /// and the keys as they would be sent now, whole or not at all: an
    /// answer that does not fit behind the replies already pending is
    /// dropped and counted in `reply_overflows`, as DECRQSS's is. A list cut
    /// at `OSC_MAX` ends before the name the cut fell in, which an
    /// even-length cut could make a different name.
    ///
    /// **XTSETTCAP is refused and counted** (`Term::tcap_sets_refused`):
    /// every retrieval takes the text's own failure path. It names an
    /// entry for xterm to "retrieve data from the terminal database", the
    /// host's, which xterm's `set_termcap` reads through `setupterm` (or
    /// `tgetent`). baud consults none, so no retrieval succeeds, and the
    /// name overrides the one XTGETTCAP uses only "If successful"
    /// (`[[source: ctlseqs]]` 535-542; `[[source: xterm-xtermcap-c]]`
    /// 48-51 and 624-657; note AM, 3 and 8.4). xterm's own code, as
    /// practice, changes no answer outside its tcap keyboard (`?1050`),
    /// which baud does not have, and never `TN`'s, which answers the
    /// `termName` resource under every keyboard (`[[source: xterm-misc]]`
    /// 5225-5226). Jev leaned to counting it, COUNT_REFUSE, 0.53/0.41.
    #[cold]
    #[inline(never)]
    pub(crate) fn tcap_end(&mut self, body: TcapBody) {
        let n = self.cold.str_len.index().min(self.strs.pending.len());
        match self.strs.pending[..n].first() {
            Some(b'q') => {
                let mut list = &self.strs.pending[1..n];
                if body == TcapBody::Cut {
                    let kept = list.iter().rposition(|&b| b == b';').unwrap_or(0);
                    list = &list[..kept];
                }
                let at = self.reply_len.index();
                match encode_tcap_reply(
                    list,
                    self.cold.replies.c1(),
                    self.cold.key_state(),
                    self.strs.tcap.name,
                    &mut self.reply[at..],
                ) {
                    Some(len) => self.reply_len = ByteLen::from_index(at + len.index()),
                    None => self.reply_overflows += 1,
                }
            }
            Some(b'p') => {
                self.strs.tcap.sets_refused = self.strs.tcap.sets_refused.next();
                self.unsupported_total += 1;
            }
            _ => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
    }

    /// Answer a DECRQSS.
    ///
    /// **1 is valid and 0 is invalid, and the DEC manuals say the opposite.**
    /// `[[source: ctlseqs]]` records the errata: "testing a DEC VT420 in 1996
    /// showed that the documented code for a valid or invalid response to
    /// DECRQSS was reversed", and vttest and DEC STD 070 agree with the tested
    /// behaviour rather than with the VT420 and VT525 manuals. baud follows
    /// xterm, which is what applications are written against. Getting it
    /// backwards would tell every application that every setting it asked
    /// about is unsupported.
    ///
    /// **Built whole, then sent whole or not at all.** It was written into the
    /// reply buffer a fragment at a time, each fragment checked on its own, so
    /// once the answer stopped fitting a fragment too long was skipped and a
    /// later, shorter one written: with 200 bytes already pending the child
    /// received a DCS with no `ST`, and with 252 `ESC P 0 ;` with the `1 $ r`
    /// gone (note AG, section 1.3). An unterminated DCS swallows every reply
    /// after it on the application's side. Now the answer is assembled in a
    /// [`DECRQSS_MAX`]-byte [`Status`] on the stack and copied under one check,
    /// as `want_reply` copies a `Report`; one that does not fit is dropped and
    /// counted once in `reply_overflows`.
    #[cold]
    #[inline(never)]
    pub(crate) fn report_status(&mut self, t: StatusTarget) {
        // The introducer and terminator follow `S7C1T`/`S8C1T` like every
        // `CSI` answer. They were written literally once, so `S8C1T` moved
        // every `CSI` reply to the eight-bit form and left DECRQSS announcing
        // itself with `ESC P` -- read as the start of something else by an
        // application that asked for eight-bit replies and parses them as
        // such. `[[source: ctlseqs]]`: `DCS` is `90` and `ST` is `9C`.
        let c1 = self.cold.replies.c1();
        let mut s = Status::new();
        s.push(c1.dcs());
        match t {
            // **A setting this core does not record answers `0$r`, not
            // silence.** `docs/vt-coverage.md`'s rule is never to answer a
            // query with the wrong shape, and "not recognised" is a shape
            // DECRQSS has. Silence would leave an application waiting for a
            // terminator that never arrives, which is worse than either
            // answer.
            //
            // **And never `1$r` over an empty body.** That is "the margins
            // are unset" to an application, rather than "I do not implement
            // that", and it is what a target that took the `1$r` prefix and
            // then matched no arm used to emit. Each arm writes its own flag
            // now, so the shape is unreachable. `DECSCUSR` was a `0$r` until
            // the cursor style was recorded and `DECSLRM` until the margins
            // were, and both left the same way: the list shrinks when a
            // feature lands, and the pin in `every_fixture_owes_the_same_reply`
            // goes red to say so: the DECRQSS fixtures are in the golden
            // capture with the replies core2 gave.
            StatusTarget::Unknown => s.push(b"0$r"),
            // **DECSCL**: `6n;0"p` or `6n;1"p`, the level and whether C1 goes
            // eight-bit, xterm's `do_dcs` shape (`[[source: xterm-misc]]`).
            // At Level 1 it is invalid: DECRQSS is not a Level 1 function,
            // and xterm answers none below its level 2 (Jev, INVALID,
            // 0.90/0.94).
            StatusTarget::ConformanceLevel => match self.cold.level {
                ConformanceLevel::One => s.push(b"0$r"),
                level => {
                    s.push(b"1$r");
                    s.push(match level.param() {
                        62 => b"62",
                        63 => b"63",
                        _ => b"64",
                    });
                    s.push(match self.cold.replies.c1() {
                        C1Transmit::EightBit => b";0\"p",
                        C1Transmit::SevenBit => b";1\"p",
                    });
                }
            },
            StatusTarget::Sgr => {
                s.push(b"1$r");
                s.sgr(self.style_raw);
                s.push(b"m");
            }
            StatusTarget::ScrollRegion => {
                s.push(b"1$r");
                s.num(self.cold.top.one_based());
                s.push(b";");
                s.num(self.cold.bot.one_based());
                s.push(b"r");
            }
            StatusTarget::CursorStyle => {
                s.push(b"1$r");
                s.dec(WireByte::of(self.cold.cursor_style.wire()));
                s.push(b" q");
            }
            // **DECSWBV and DECSMBV, as the controls that set them**, the
            // `Ps` of each band on which DEC and xterm agree
            // (`BellVolume::reported`): the VT520's DECRPSS list names both
            // (`[[source: vt520-rm]]` pp. 5-103 and 5-104).
            StatusTarget::WarningBellVolume => {
                s.push(b"1$r");
                s.dec(WireByte::of(self.cold.bell.warning.reported()));
                s.push(b" t");
            }
            StatusTarget::MarginBellVolume => {
                s.push(b"1$r");
                s.dec(WireByte::of(self.cold.bell.margin.reported()));
                s.push(b" u");
            }
            StatusTarget::Protection => {
                s.push(b"1$r");
                s.dec(WireByte::of(u8::from(self.cold.protect.is_protected())));
                s.push(b"\"q");
            }
            StatusTarget::SideMargins => {
                s.push(b"1$r");
                s.num(self.cold.left.one_based());
                s.push(b";");
                s.num(self.cold.right.one_based());
                s.push(b"s");
            }
            // **DECSCPP answers the page the grid is**: STD 070's DECRQSS
            // lists "Set Columns Per Page DECSCPP $ |" (`[[source:
            // dec-std-070]]`, p. 5-196), and an answer is a control the
            // application sends back. By DECSCPP's own rule, so a grid of
            // 100 is 132, "the next higher supported number"; and the
            // grid's, not the width last asked for, which an embedder that
            // did not resize never gave the page. xterm answers from
            // `IN132COLUMNS`, the last asked (`[[source: xterm-misc]]`):
            // the record's open question.
            StatusTarget::ColumnsPerPage => s.push(match PageWidth::page_of(self.cols) {
                PageWidth::Eighty => b"1$r80$|",
                PageWidth::OneThirtyTwo => b"1$r132$|",
            }),
            // **DECSLPP answers the lines the screen is, and 24 below 24**:
            // STD 070's DECSLPP algorithm makes any `Pn` up to 24 a page of
            // 24 (p. 5-76), so a shorter screen's page sent back is
            // `24 t`; and `CSI Ps t` below 24 is an XTWINOPS (`[[source:
            // ctlseqs]]`), so the screen's own count there would be a
            // wrong-shaped answer (Jev, FLOOR, 0.96/0.94). xterm floors at
            // 24 too, from its last row's index, one short (`[[source:
            // xterm-misc]]`); the count here is the screen's.
            StatusTarget::LinesPerPage => {
                let last = Row::from_index(self.rows.index().saturating_sub(1)).one_based();
                s.push(b"1$r");
                match last.get() < 24 {
                    true => s.push(b"24"),
                    false => s.num(last),
                }
                s.push(b"t");
            }
            // **DECSACE**, listed by `[[source: vt420-rm]]` Table 12-4 and
            // the VT510's DECRQSS ("Select Attribute Change Extent DECSACE
            // *x"): the extent DECCARA and DECRARA use, `0` for the stream,
            // which `1` selects too, and `2` for the rectangle.
            // **DECSSDT and DECSASD**, "may be queried through DECRQSS"
            // (`[[source: dec-std-070]]` 070-14 p. 14-9), as the controls
            // that set them: the type, and the display DECSASD selected.
            StatusTarget::StatusType => {
                s.push(b"1$r");
                s.dec(WireByte::of(self.strs.status.kind.code()));
                s.push(b"$~");
            }
            StatusTarget::ActiveDisplay => {
                s.push(b"1$r");
                s.dec(WireByte::of(self.strs.status.active.code()));
                s.push(b"$}");
            }
            StatusTarget::AttrExtent => s.push(match self.cold.attr_extent {
                AttrExtent::Stream => b"1$r0*x",
                AttrExtent::Rectangle => b"1$r2*x",
            }),
            // **XTMODKEYS and XTFMTKEYS, answered as the controls that set
            // them**: "replacing the Pt with the corresponding CSI string"
            // (`[[source: ctlseqs]]`), so `> Pp ; Pv m` and `> Pp ; Pv f`,
            // every resource. A disabled one is `-1` and modifyOtherKeys'
            // mask `: mask`, as xterm's `do_dcs` writes them (`[[source:
            // xterm-misc]]`; Jev's lean, XTERM_MINUS_ONE, 0.55/0.82 then
            // 0.23/0.27). xterm spells the format answer `>%d;%dm`, an
            // XTMODKEYS, which would set the other resource: a departure not
            // followed.
            StatusTarget::KeyModify(r) => {
                let v = self.cold.keys.modify(r);
                s.push(b"1$r>");
                s.dec(WireByte::of(r.pp()));
                s.push(b";");
                match u8::try_from(v.get()) {
                    Ok(n) => s.dec(WireByte::of(n)),
                    Err(_) => s.push(b"-1"),
                }
                let mask = self.cold.keys.ignore();
                if r == KeyResource::Other && !mask.is_none() {
                    s.push(b":");
                    s.dec(WireByte::of(mask.wire() - 1));
                }
                s.push(b"m");
            }
            // **XTSMTITLE, answered as the control that sets the modes in
            // force**: "replacing the Pt with the corresponding CSI string"
            // (`[[source: ctlseqs]]`), `> 0 ; 2 t`, and `> t` with none
            // set, which sent back is XTSMTITLE's default, none set. xterm's
            // `do_dcs` writes one 0 or 1 a mode (`[[source: xterm-misc]]`),
            // which sent back sets the wrong modes: a departure not followed
            // (Jev's lean, SETLIST, 0.70/0.76). A `> Ps t` naming one mode
            // gets the same answer.
            StatusTarget::TitleModes => {
                s.push(b"1$r>");
                for (i, m) in self.strs.title_modes.set().enumerate() {
                    if i > 0 {
                        s.push(b";");
                    }
                    s.dec(WireByte::of(m.code()));
                }
                s.push(b"t");
            }
            StatusTarget::KeyFormat(r) => {
                s.push(b"1$r>");
                s.dec(WireByte::of(r.pp()));
                s.push(b";");
                s.dec(WireByte::of(self.cold.keys.format(r).code()));
                s.push(b"f");
            }
        }
        s.push(c1.st());
        let at = self.reply_len.index();
        match s.bytes() {
            Some(b) if at + b.len() <= self.reply.len() => {
                self.reply[at..at + b.len()].copy_from_slice(b);
                self.reply_len = ByteLen::from_index(at + b.len());
            }
            // **A truncated reply is a wrong answer**, so a reply that does
            // not fit is dropped whole and counted once -- the rule
            // `want_reply`, `Term::queue` and `Term::key` follow.
            _ => self.reply_overflows += 1,
        }
    }
}

impl Paint<'_, '_> {
    /// An SGR sequence for `w`, `CSI 0 ; Ps... m`, XTREPORTSGR's answer:
    /// built by [`Status::sgr`], the encoder DECRQSS's SGR answer uses, and
    /// sent whole or dropped and counted as that answer is. The introducer
    /// follows `S7C1T`/`S8C1T`.
    #[cold]
    #[inline(never)]
    pub(crate) fn report_rendition(&mut self, w: StyleWord) {
        let mut s = Status::new();
        s.push(self.cold.replies.c1().csi());
        s.sgr(w);
        s.push(b"m");
        let at = self.reply_len.index();
        match s.bytes() {
            Some(b) if at + b.len() <= self.reply.len() => {
                self.reply[at..at + b.len()].copy_from_slice(b);
                self.reply_len = ByteLen::from_index(at + b.len());
            }
            _ => self.reply_overflows += 1,
        }
    }
}

/// A DECRQSS answer, assembled before any of it reaches the reply buffer.
///
/// [`DECRQSS_MAX`] bytes on the stack of a `#[cold]` function, rather than
/// written in place behind the pending reply and committed or rolled back:
/// the answer's room is then the encoder's to prove, once, at build time, and
/// does not depend on how large an embedder made the reply buffer.
struct Status {
    buf: [u8; DECRQSS_MAX],
    /// How much of `buf` is the answer, or `None` once a part did not fit.
    /// `None` cannot happen while [`DECRQSS_MAX`] counts every part, and if
    /// the encoder ever outgrew it the answer would be dropped and counted
    /// rather than sent short.
    len: Option<ByteLen>,
}

impl Status {
    fn new() -> Status {
        Status {
            buf: [0; DECRQSS_MAX],
            len: Some(ByteLen::ZERO),
        }
    }

    /// The whole answer, or `None` if any part of it was refused.
    fn bytes(&self) -> Option<&[u8]> {
        self.len.map(|n| &self.buf[..n.index()])
    }

    fn push(&mut self, bytes: &[u8]) {
        let Some(n) = self.len else {
            return;
        };
        let at = n.index();
        self.len = match self.buf.get_mut(at..at + bytes.len()) {
            Some(dst) => {
                dst.copy_from_slice(bytes);
                Some(ByteLen::from_index(at + bytes.len()))
            }
            None => None,
        };
    }

    /// The current rendition as an SGR parameter string, always starting at 0.
    ///
    /// **The leading `0` is not padding.** DECRQSS answers with something the
    /// application can send back verbatim, and a string that only *adds*
    /// attributes would not reproduce this state on a terminal that had others
    /// set.
    fn sgr(&mut self, w: StyleWord) {
        self.push(b"0");
        for (bit, code) in Attrs::WIRE {
            if w.attrs().has(bit) {
                self.push(b";");
                // The three colon styles have no one-parameter spelling: their
                // `WIRE` code is the sub-parameter, written as `4:n` so the
                // answer round-trips through `SgrAttr::parse`'s colon arm.
                if colon_style(bit) {
                    self.push(b"4:");
                }
                self.dec(WireByte::of(code));
            }
        }
        // `SGR 10` is the primary font and is the default, so it is emitted
        // only when it is not -- a redundant `10` is noise a comparison
        // against the default would fail on.
        if !w.font().is_primary() {
            self.push(b";");
            self.dec(WireByte::of(w.font().sgr()));
        }
        if !w.underline().is_default() {
            self.push(b";");
            self.colour(w.underline(), WireByte::of(58));
        }
        let (fg, bg) = (w.fg(), w.bg());
        if !fg.is_default() {
            self.push(b";");
            self.colour(fg, WireByte::of(38));
        }
        if !bg.is_default() {
            self.push(b";");
            self.colour(bg, WireByte::of(48));
        }
    }

    fn colour(&mut self, c: ColorWord, base: WireByte) {
        if let Some((r, g, b)) = c.as_rgb() {
            self.dec(base);
            self.push(b";2;");
            self.dec(WireByte::of(r));
            self.push(b";");
            self.dec(WireByte::of(g));
            self.push(b";");
            self.dec(WireByte::of(b));
        } else if let Some(i) = c.as_index() {
            self.dec(base);
            self.push(b";5;");
            self.dec(WireByte::of(i));
        }
    }

    /// A 1-based coordinate as decimal. Wider than `dec`, because a margin
    /// can be past column 255.
    fn num(&mut self, v: OneBased) {
        const D: &[u8; 10] = b"0123456789";
        let mut buf = [0u8; 5];
        let mut i = buf.len();
        let mut n = v.get();
        loop {
            i -= 1;
            buf[i] = D[usize::from(n % 10)];
            n /= 10;
            if n == 0 {
                break;
            }
        }
        let tail = buf;
        self.push(&tail[i..]);
    }

    /// A wire code as decimal, at most three digits.
    ///
    /// Takes a [`WireByte`] rather than a `u8` so it cannot be handed a
    /// coordinate: a right margin can sit past column 255 and this saturates
    /// there, writing the wrong number rather than failing. `num` is the one
    /// for those, and the types are what keep them apart.
    fn dec(&mut self, v: WireByte) {
        const D: &[u8; 10] = b"0123456789";
        let mut buf = [0u8; 3];
        let mut i = buf.len();
        let mut n = v.get();
        loop {
            i -= 1;
            buf[i] = D[usize::from(n % 10)];
            n /= 10;
            if n == 0 {
                break;
            }
        }
        let tail = buf;
        self.push(&tail[i..]);
    }
}
