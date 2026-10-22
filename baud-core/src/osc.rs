//! Operating System Commands: titles, the palette, hyperlinks, the clipboard.
//!
//! **The parsing is `baud_domain::Osc`.** What is here is the wiring --
//! accumulate a body, decide what it was, write the result somewhere the
//! embedder can read -- and that is the half this core has to get right on its
//! own. A core that recognised `OSC 8` and never set the link index would open
//! every URI it was sent and link no cell, and no test of the parser could see
//! it.
//!
//! **The two pinned differentials were structurally blind to all of this.**
//! The glyph differential read content and the rendition one read the
//! resolved `StyleWord`; a window title changes no cell and a link index rides
//! in `CellFlags`, which neither read. So the gate for this file was the
//! control-string differential against `baud-core2`, which compared the title,
//! the icon, the palette, the link index of a written cell, the URI it
//! resolves to, and the two refusal counters -- and it was perturbed both ways
//! before it was believed. All three went with that crate (bcefaae). The
//! golden capture that replaced them keeps cells, the cursor and the reply,
//! and no title, icon, palette or link, so what holds this file now is chiefly the
//! hand-written suites, `tests/xterm/osc.rs` and `tests/xterm/hyperlinks.rs`.

use crate::paint::Paint;
use crate::storage::{LINK_MAX, NLINK, OSC_MAX, StyleLog};
use crate::term::Cold;
use baud_domain::*;

impl Paint<'_, '_> {
    /// Begin a control string: record which one, and empty the accumulator.
    ///
    /// **Reset here, at the introducer, and not at the terminator.** A string
    /// spans reads, so the accumulator outlives a `feed`; and a string that
    /// `CAN` or `SUB` cancels never reaches a terminator at all. Resetting
    /// where the string starts is one rule that covers both, and the
    /// alternative -- reset on completion, plus a second rule for cancellation
    /// -- is the shape that made `pre: Option<Classified>` break chunk
    /// invariance: a value carried between two functions and consumed by a
    /// later, different one, because the path has more exits than it looks
    /// like it has.
    ///
    /// **And the encoding it began under**, which `str_put` reads to keep an
    /// `OSC`'s or a Sixel's GR byte as its 7-bit byte under ISO 8859-1: taken once here,
    /// in the introducer's arm, rather than passed in the body's arm on
    /// every byte of every string, a Sixel's included.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_start(&mut self, b: StreamByte, enc: InputEncoding) {
        // **A Sixel body that never reached its end lets its claim go
        // here.** `CAN`, an `ESC` that is not `ST` and a C1 place what it
        // received (`Paint::sixel_cut`) and `SUB` is a blank sixel, so none
        // of them leaves one; this stays for a body no byte ended. Nothing
        // to do after a body that ended.
        if self.cold.str_kind == StrKind::Sixel {
            self.strs.sixel.abandon(self.images);
        }
        self.cold.str_kind = StrKind::of(b);
        self.cold.str_enc = enc;
        self.cold.str_len = ByteLen::ZERO;
    }

    /// **What a `StrPut` takes: one body byte, or a kitty payload's run.**
    /// Out of line, so the parser's loop holds a call and nothing else.
    ///
    /// A kitty body is base64 behind a line of keys, and fed a byte at a
    /// time it cost about 4 ns a byte: a 1920x1080 `f=32` picture took
    /// 46.0-47.1 ms, all of it here, and 4.7 ms a run at a time
    /// (`docs/record.md`, "A kitty payload is fed a run at a time"). So its
    /// printable-ASCII run is found as the text path finds one (`run_of_text`, the same vector scan) and handed
    /// to the stream whole. **The run is a subset of what the byte path
    /// would put, edge for edge**: `20-7E` in `Node::SosStr` is `StrPut`
    /// back to `SosStr` under both encodings
    /// (`graph::classes::a_kitty_run_is_bytes_the_string_body_puts`), and
    /// `gr_as_gl` maps none of them. Everything else -- the `ESC` that may
    /// begin `ST` (`[[source: ctlseqs]]` 201) or cancel the string
    /// (`[[source: williams]]`), `CAN`, `SUB`, the C0 the body ignores,
    /// `DEL`, a GR byte, `9C` under ISO 8859-1 -- ends the run where it
    /// stands and takes its own edge, so the run is bounded by the read and
    /// by the first byte the graph treats otherwise.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_run(&mut self, from: BytePos, left: ReadLen) -> ReadLen {
        // A ReGIS body byte may stop the read at a delay (`regis_byte`).
        if self.cold.str_kind == StrKind::Regis {
            return self.regis_byte(from, left);
        }
        if self.cold.str_kind == StrKind::Kitty {
            let n = crate::write::run_of_text(self, from, left);
            if n.index() > 0 {
                self.kitty_run(from, n);
                return ReadLen::of_bytes(n);
            }
        }
        self.str_put(StreamByte::from_stream(self.arena[from.index()]));
        ReadLen::ONE
    }

    /// One body byte, truncating rather than growing.
    #[inline(always)]
    fn str_put(&mut self, b: StreamByte) {
        // A Sixel body is decoded as it arrives, a byte at a time, into the
        // gap of the image arena its `q` claimed (`SixelStream`): no copy of
        // it is kept, so no length limits it, and its pixels do.
        if self.cold.str_kind == StrKind::Sixel {
            // **A C0 is dropped here, before the decoder, and a `BEL`
            // rings.** The graph puts every C0 of a DCS body,
            // `[[source: williams]]`'s `dcs passthrough` row (388), and the
            // decoder reads a number up to its first non-digit, so any of them
            // inside one split it: `!1 BEL 0~` drew one column where `!10~` draws
            // ten, and `#1;2;10 BEL 0;0;0` set red to 10%. DEC's own Sixel text
            // gives the format effectors no meaning: `[[source: dec-std-070]]`
            // 070-9 §9.1, "These codes shall have no effect on the interpretation
            // of Sixel Control strings" (PDF p. 925), and of the rest: "4. BEL -
            // same action as in text mode. 5. ENQ - same action as in text
            // mode. All other C0 Codes are ignored" (926). Text mode counts a
            // bell for `BEL` and does nothing for `ENQ`, baud keeping no
            // answerback, so `ENQ` is dropped with the rest
            // (`Paint::sixel_put_rare`). `[[source: vt3xx-sixel]]` says nothing
            // of C0. xterm skips `HT LF VT FF CR` as white space and ends a
            // number on `BS`, its departure (`[[source: xterm-graphics-sixel]]`
            // 541-555), and rings a `BEL` without handing it to its parser
            // (`[[source: xterm-reset]]`'s file, 3408-3412 and 3691-3705).
            // `DEL` never gets here, the graph ignoring it in a DCS body as
            // williams' row does and as 070-9 §9.2 lists for Sixel, "7/15
            // (DELETE) Ignored" (926); xterm hands its parser one, which ends a
            // number there, its departure.
            //
            // **Under ISO 8859-1 a GR byte is its 7-bit byte, and a `SPACE` is
            // dropped with the C0** (`Sixel::body_byte`, which quotes 070-9
            // §8.1, §9.2 and §9.4). The body stored GR raw until 2026-10-13,
            // and the decoder skipped it, so `FE FE` drew nothing; and a
            // `SPACE` reached the decoder, which ended a number at it, so `!1
            // SPACE 0~` drew one column where `!10~` draws ten. xterm skips
            // `isspace` bytes before it reads a digit
            // (`[[source: xterm-graphics-sixel]]` 541-547), as this does. The
            // encoding is the one the introducer began under (`str_start`).
            //
            // `21-7F` is tested here and the rest out of line, in
            // `sixel_put_rare`, so the common byte costs what the `is_c0` this
            // replaced cost (`Sixel::body_byte_is_gl_past_space`); the `DEL`
            // in that range never arrives, the graph ignoring it.
            if !Sixel::body_byte_is_gl_past_space(b) {
                return self.sixel_put_rare(b);
            }
            return self.sixel_put(b);
        }
        // **An `APC` whose body begins `G` is a kitty graphics command**
        // (`[[source: kitty-graphics]]`, "The graphics escape code": "All
        // graphics escape codes are of the form: <ESC>_G<control
        // data>;<payload><ESC>\"), routed by its first byte as `52;` routes a
        // clipboard string, so the graph is unchanged. Under ISO 8859-1 a GR
        // byte of it is its 7-bit byte, the rule `[[source: dec-std-070]]`
        // 3.5.4.5 gives APC and the paragraph below quotes. Any other `APC`
        // is `Other`, kept and discarded unread as before.
        if self.cold.str_kind == StrKind::Kitty {
            return self.kitty_put(self.cold.str_enc.gr_as_gl(b));
        }
        if self.cold.str_kind == StrKind::Apc {
            if self.cold.str_enc.gr_as_gl(b).raw_byte() == b'G' {
                return self.kitty_start();
            }
            self.cold.str_kind = StrKind::Other;
        }
        // **DECRQSS and DECRSPS drop every C0 before their data is kept,
        // and read a GR byte as its 7-bit byte under ISO 8859-1.** The graph
        // puts every C0 of a DCS body for the handler to judge
        // (`graph::string`), and these give none a meaning:
        // `[[source: dec-std-070]]` 3.5.4 says the format effectors' inclusion
        // "is intended for convenience in formatting and storing control
        // strings" and recommends "that these characters not affect the
        // interpretation of the control string" (PDF p. 133), and 3.5.4.4
        // "that all other control codes not used within a command string be
        // ignored by hardware"; of DEC's practice `[[source: williams]]` says
        // a C0 with no meaning to the device function is "silently ignored"
        // (818-822). So `m CR`, `m BEL` and `m NUL` name SGR (Jev,
        // B_PUT_ALL, 0.99/0.99). xterm's `strcmp` against `m` fails on any
        // (`[[source: xterm-misc]]`'s file, 4935), practice and not followed;
        // `[[source: ctlseqs]]` leaves error recovery to the implementation
        // (133-137). The data of either is a control sequence's characters
        // ("the identifying intermediate and final characters", DECRQSS, p.
        // 5-196), whose GR STD 070 3.5.3 g reads as "the character from the
        // corresponding row in columns 02 to 07" (PDF p. 129); `[[source:
        // ecma48]]` §9 gives GR in "the contents of the command string" "the
        // same meanings as" 02/00-07/14, and williams (462) and ctlseqs read
        // GR as GL in a control string, as xterm's `AsciiOf` does; so `DCS $
        // q ED ST`
        // names SGR, and `FF`, read as `7F`, is dropped as a `DEL` is (Jev's
        // lean STRIP, 0.60/0.56, the texts' reading). Both were kept raw, so
        // the request named nothing and answered invalid. Under UTF-8
        // `gr_as_gl` maps nothing: the bytes are a character's.
        if matches!(
            self.cold.str_kind,
            StrKind::Decrqss
                | StrKind::RestoreCursor
                | StrKind::RestoreTabs
                | StrKind::LocatorKeys
                | StrKind::Tcap
                | StrKind::TcapCut
                | StrKind::RestoreTerminal
                | StrKind::RestoreColors
        ) {
            return self.status_data_put(b);
        }
        // **Under ISO 8859-1 an `OSC` keeps a GR byte as its 7-bit byte**,
        // its clipboard body included. `[[source: dec-std-070]]` 3.5.4.5: "GR
        // (8-bit) graphic characters in APC, OSC, and PM control strings will
        // be treated as their 7-bit equivalent (the eighth bit will be
        // ignored)"; `[[source: ecma48]]` §9 gives "the contents of the
        // command string" "the same meanings" as `02/00` to `07/14`; and
        // `[[source: williams]]` treats GR as GL "in all states" (462). The
        // body kept the byte as it came, so `ESC ] B0 BB t BEL` named no
        // `OSC`, a title in GR was stored in GR, and `B5 B2 BB` was not the
        // `52;` that routes a copy below. Above this, a Sixel folds its own,
        // as 070-9 §9.4 says, and a DECRQSS keeps its `Pt`'s, the owner's
        // open call (`dcs_passthrough`, `docs/vt-coverage.md`). Below it, `Other` is a
        // DCS no handler reads or an `APC`, `PM` or `SOS`, kept and dropped
        // unread at the terminator, so a strip there would move nothing; an
        // `APC` or `PM` that gains a reader takes the rule then. Under UTF-8
        // `gr_as_gl` maps nothing: the bytes are a character's.
        let b = match self.cold.str_kind {
            StrKind::Osc => match self.cold.str_enc.gr_as_gl(b) {
                folded if folded == b => b,
                folded => self.osc_gr(b, folded),
            },
            StrKind::Clipboard | StrKind::Notice | StrKind::ColorControl => {
                self.cold.str_enc.gr_as_gl(b)
            }
            _ => b,
        };
        // An `OSC 52` body past its `52;`: decoded into the clipboard areas,
        // not kept in `pending`, whose 512 bytes hold under 400 of text.
        if self.cold.str_kind == StrKind::Clipboard {
            return self.clip_put(b);
        }
        // A kitty `OSC 99` payload, past the metadata `pending` keeps.
        if self.cold.str_kind == StrKind::Notice {
            return self.notice_put(b);
        }
        // A kitty `OSC 21` past its `21;`, read a record at a time.
        if self.cold.str_kind == StrKind::ColorControl {
            return self.colour_put(b);
        }
        self.keep_pending(b);
    }

    /// **A GR byte of an `OSC` body under ISO 8859-1**: its 7-bit byte, as
    /// above, unless XTSMTITLE's UTF-8 mode is set and the body names a
    /// title or an icon name (`0;`, `1;` or `2;`), whose GR bytes are then
    /// kept for [`Paint::set_labels`] to read as UTF-8, as xterm's parser
    /// keeps a title's eight bits under its `utf8Title` (`[[source:
    /// xterm-reset]]`'s file, `IsSetUtf8Title`). The text's "window/icon
    /// labels" names the icon too, where xterm keeps `0` and `2` alone
    /// (Jev's lean, BUILD_TEXT: after a second ask the orders split,
    /// BUILD_XTERM at 0.38 in one and BUILD_TEXT at 0.82 in the other, and
    /// the one with the higher mean probability was taken).
    /// Out of line: only a GR byte under ISO 8859-1 reaches it.
    #[cold]
    #[inline(never)]
    fn osc_gr(&self, b: StreamByte, folded: StreamByte) -> StreamByte {
        let kept = &self.strs.pending[..self.cold.str_len.index()];
        match self.strs.title_modes.has(TitleMode::SetUtf8) && crate::window::names_a_label(kept) {
            true => b,
            false => folded,
        }
    }

    /// An `OSC 0`, `1` or `2` stored: the title, the icon name or both, as
    /// XTSMTITLE's modes read it. **In hexadecimal, while that mode is
    /// set**, decoded as xterm's `ChangeGroup` decodes it (`[[source:
    /// xterm-misc]]`, [`decode_hex_title`]), and a body that is not even
    /// hexadecimal changes nothing and is counted. The encoding the bytes
    /// are in is kept beside them ([`crate::window::label_encoding`]), so
    /// the embedder reads a name as it was sent rather than in whatever
    /// encoding is in force later.
    fn set_labels(&mut self, t: &[u8], which: Labels) {
        let mut decoded = [0u8; OSC_MAX / 2];
        let sent = match self.strs.title_modes.has(TitleMode::SetHex) {
            false => crate::window::Spelled::Text,
            true => crate::window::Spelled::Hex,
        };
        let bytes = match sent {
            crate::window::Spelled::Text => t,
            crate::window::Spelled::Hex => match decode_hex_title(t, &mut decoded) {
                Some(d) => d,
                None => {
                    self.unsupported_param += 1;
                    return;
                }
            },
        };
        let enc =
            crate::window::label_encoding(self.cold.str_enc, self.strs.title_modes, sent, bytes);
        if which != Labels::Icon {
            self.title_len = copy_bounded(self.strs.title, bytes);
            self.strs.title_enc = enc;
        }
        if which != Labels::Title {
            self.icon_len = copy_bounded(self.strs.icon, bytes);
            self.strs.icon_enc = enc;
        }
    }

    /// **`OSC 3`: the window's name or its icon's, as the property's type
    /// reads it**, and every other property counted and dropped (the
    /// owner's call of 2026-10-20, note AV). `WM_NAME` and `_NET_WM_NAME`
    /// set the one title, `WM_ICON_NAME` and `_NET_WM_ICON_NAME` the one
    /// icon name, the last write winning (Jev, ICCCM_AND_EWMH, 0.96/0.98):
    /// ICCCM's two in ISO 8859-1, the `STRING` xterm writes them as, and
    /// EWMH's two in UTF-8, their `UTF8_STRING`, a value that is not UTF-8
    /// being none (Jev, BY_PROPERTY_TYPE, 0.97/1.00). The bytes are taken as
    /// sent: xterm's `ChangeXprop` goes past `ChangeGroup`, so no
    /// hexadecimal title mode (`[[source: xterm-misc]]`). A bare name
    /// deletes the property, which leaves the window or its icon unnamed.
    /// **Refused and counted while the window operations are off**, as
    /// xterm gates it under `allowWindowOps` (`AllowWindowOps(xw,
    /// ewSetXprop)`; Jev's lean, FOLLOWS_WINDOW_OPS, 0.38/0.52).
    fn set_property(&mut self, p: XProperty<'_>) {
        if self.strs.window_asks.ops == WindowOps::Off {
            self.strs.window_asks.refuse();
            return;
        }
        let (name, value) = match p {
            XProperty::Set(name, value) => (name, value),
            XProperty::Delete(name) => (name, &b""[..]),
            XProperty::Unmapped => return self.drop_property(),
        };
        let enc = match name.encoding() {
            PropertyType::String => InputEncoding::Latin1,
            PropertyType::Utf8String if core::str::from_utf8(value).is_ok() => InputEncoding::Utf8,
            PropertyType::Utf8String => return self.drop_property(),
        };
        match name.names() {
            NamedLabel::Window => {
                self.title_len = copy_bounded(self.strs.title, value);
                self.strs.title_enc = enc;
            }
            NamedLabel::Icon => {
                self.icon_len = copy_bounded(self.strs.icon, value);
                self.strs.icon_enc = enc;
            }
        }
    }

    /// An `OSC 3` property no window here has, counted as unsupported too.
    fn drop_property(&mut self) {
        self.strs.properties_dropped = self.strs.properties_dropped.next();
        self.unsupported_total += 1;
    }

    /// Keep one body byte for the terminator, counting one past `OSC_MAX`.
    #[inline(always)]
    fn keep_pending(&mut self, b: StreamByte) {
        let n = self.cold.str_len.index();
        if n < OSC_MAX {
            self.strs.pending[n] = b.raw_byte();
            self.cold.str_len = ByteLen::from_index(n + 1);
            // **`52;` routes the rest of the body to the clipboard**, whose
            // `Pd` is unbounded: kept here, a copy past 380-odd octets would
            // be cut at `OSC_MAX`. Matched on the spelling `Osc::parse`
            // matches, so no body it would call `52` stays behind.
            if n == 2 && self.cold.str_kind == StrKind::Osc && self.strs.pending[..3] == *b"52;" {
                self.clip_start();
            }
            // **`21;` routes the rest to kitty's colour control**, read a
            // record at a time, so a sequence of any length is honoured
            // (`colour.rs`).
            if n == 2 && self.cold.str_kind == StrKind::Osc && self.strs.pending[..3] == *b"21;" {
                self.colour_start();
            }
            // **`99;` and then its metadata's `;` route the payload to the
            // notifications' chunk**, which holds the 4096 bytes kitty lets
            // a chunk carry (`notice.rs`).
            if b.raw_byte() == b';'
                && n >= 3
                && self.cold.str_kind == StrKind::Osc
                && self.strs.pending[..3] == *b"99;"
            {
                self.notice_start();
            }
        } else {
            self.string_truncations += 1;
        }
    }

    /// One byte of a DECRQSS or DECRSPS body: a C0 or a `DEL` dropped, a
    /// GR byte read as its 7-bit byte under ISO 8859-1 (`str_put`), and the
    /// rest kept for the terminator, or, for DECTABSR, read as it arrives.
    fn status_data_put(&mut self, b: StreamByte) {
        let b = self.cold.str_enc.gr_as_gl(b);
        if b.is_c0() || b.raw_byte() == 0x7f {
            return;
        }
        if self.cold.str_kind == StrKind::RestoreTabs {
            return self.restore_tab_put(b);
        }
        if self.cold.str_kind == StrKind::RestoreColors {
            return self.restore_color_put(b);
        }
        // A byte past `OSC_MAX` cuts an XTGETTCAP list, which `keep_pending`
        // counts; the kind remembers it for the terminator.
        if self.cold.str_kind == StrKind::Tcap && self.cold.str_len.index() == OSC_MAX {
            self.cold.str_kind = StrKind::TcapCut;
        }
        self.keep_pending(b);
    }

    /// Decode one Sixel body byte the handler keeps.
    #[inline]
    fn sixel_put(&mut self, b: StreamByte) {
        self.strs.sixel.put(b, self.images);
    }

    /// A Sixel body byte outside `21-7E`: kept, folded or dropped as
    /// `Sixel::body_byte` says, under the encoding the introducer began
    /// under.
    #[cold]
    #[inline(never)]
    fn sixel_put_rare(&mut self, b: StreamByte) {
        // 070-9 §9.1, "BEL - same action as in text mode" (`str_put`).
        if b.is(StreamByte::BEL) {
            self.bells += 1;
            return;
        }
        // 070-9 §9.1, "SUB - process as if a 3/15 code (blank Sixel) was
        // received" (`Node::SixelStr`). One call to the decoder: a second
        // made the compiler inline its ground walk here and move its data
        // step out of line, a quarter of a percent on `sixel-cat`.
        let kept = match b.is(StreamByte::SUB) {
            true => Some(StreamByte::from_stream(b'?')),
            false => Sixel::body_byte(self.cold.str_enc, b),
        };
        if let Some(b) = kept {
            self.sixel_put(b);
        }
    }

    /// **A Sixel cut off by `CAN`, an `ESC` that is not `ST` or a C1** is
    /// placed from what it received, as at `ST`: williams, "Cancelling a
    /// control string does not undo these effects" (501-503), and its
    /// `unhook` lets the handler "finish neatly" (728-729); `[[source:
    /// dec-std-070]]` 070-9 prints "each Sixel in received order" (§5) and
    /// on exit updates the text position "to reflect the vertical motion
    /// commands executed in the Sixel printing" (§11.3). Jev, KEEP,
    /// 0.94/0.94 for `ESC` and a C1, and the lean KEEP, 0.72/0.68, for
    /// `CAN` (2026-10-19). Until then the image was discarded and its claim
    /// let go at the next string.
    #[cold]
    #[inline(never)]
    pub(crate) fn sixel_cut(&mut self) {
        if self.cold.str_kind == StrKind::Sixel {
            self.sixel_end();
            self.cold.str_kind = StrKind::Other;
        }
    }

    /// The string is complete. Act on it if it is one this core acts on.
    ///
    /// Takes the terminating byte, because `[[source: ctlseqs]]` says a reply
    /// "uses the same terminator used in a query" -- so `BEL` and `ST` are one
    /// action with two shapes rather than two actions, and which one arrived
    /// has to travel with it.
    #[cold]
    #[inline(never)]
    pub(crate) fn str_end(&mut self, b: StreamByte) {
        let term = match b.raw_byte() {
            0x07 => StringTerm::Bel,
            _ => StringTerm::St,
        };
        match self.cold.str_kind {
            StrKind::Osc => self.dispatch_osc(term),
            StrKind::Sixel => self.sixel_end(),
            StrKind::Clipboard => self.clip_end(),
            StrKind::Notice => self.notice_end(),
            StrKind::ColorControl => self.colour_end(),
            StrKind::Decrqss => {
                let n = self.cold.str_len.index();
                let target = StatusTarget::parse(&self.strs.pending[..n]);
                self.report_status(target);
            }
            // `SOS`, `PM` and `APC` have no defined content here, and `DCS`
            // has no prefix parser yet. **Consumed is the part that matters**:
            // a body that reached the screen as text would render an
            // application's private protocol as glyphs.
            StrKind::Kitty => self.kitty_end(),
            StrKind::RestoreCursor => {
                // A copy of the head of the body: the data string is at most
                // `CIR_DATA_MAX` bytes, and anything past the four
                // designations is not read.
                let n = self.cold.str_len.index().min(CIR_COPY);
                let mut data = [0u8; CIR_COPY];
                data[..n].copy_from_slice(&self.strs.pending[..n]);
                self.restore_cursor_information(&data[..n]);
            }
            StrKind::RestoreTabs => self.restore_tab_end(),
            // Restored where the terminator is read (`write.rs`), since a
            // control it replays can move the parser's node.
            StrKind::RestoreTerminal => {}
            StrKind::RestoreColors => self.restore_color_end(),
            // ReGIS acts as its body arrives and keeps its state for the
            // next envelope; its cursors go with the envelope, "the ReGIS
            // locator will be disabled until ReGIS is re-entered" (070-13
            // § 5.2) and the output cursor shows only "in ReGIS mode".
            // The first surface is displayed again here, before any text
            // that follows, as the VT340's coupled pages have it; a cancel
            // displays it at the read's end (`Term::regis_read_ends`).
            StrKind::Regis => {
                self.regis_cursor_off();
                crate::regis::show_first(self.images, self.strs.regis_page, self.strs.regis);
            }
            StrKind::LocatorKeys => self.locator_keys_end(),
            StrKind::Tcap => self.tcap_end(crate::dcs::TcapBody::Whole),
            StrKind::TcapCut => self.tcap_end(crate::dcs::TcapBody::Cut),
            StrKind::Other | StrKind::Apc => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
        self.cold.str_len = ByteLen::ZERO;
    }

    /// A palette entry written, and the colour epoch moved when it changed:
    /// a renderer resolves the table again under a new epoch, and a write
    /// that leaves the entry as it was must not make it.
    pub(crate) fn set_palette(&mut self, i: Chan, w: ColorWord) {
        if self.strs.palette[i.index()] != w {
            self.strs.palette[i.index()] = w;
            self.style_log.colors_moved();
        }
    }

    /// Every `OSC 4` override cleared, which is every entry a rendition
    /// resolves through: `OSC 104` with no parameter, and both resets, as
    /// xterm's `ReallyReset` calls `ResetAnsiColorRequest` in its common path
    /// (`[[source: xterm-reset]]`). The dynamic colours are not touched --
    /// neither xterm path resets them. Through [`Paint::set_palette`], so the
    /// colour epoch moves only if some entry was overridden.
    pub(crate) fn reset_palette(&mut self) {
        clear_palette(self.strs.palette, self.style_log);
        clear_specials(self.strs.palette, self.style_log);
    }

    /// One special colour written, the colour epoch moved when it changed:
    /// renditions resolve through it while its mode is on.
    pub(crate) fn set_special(&mut self, which: SpecialColor, w: ColorWord) {
        let at = crate::storage::SPECIAL_PALETTE + which.index();
        if self.strs.palette[at] != w {
            self.strs.palette[at] = w;
            self.style_log.colors_moved();
        }
    }

    /// One `c ; spec` naming a special colour, set or asked for; a spec no
    /// reader takes is counted, as `OSC 4`'s is.
    fn special_pair(
        &mut self,
        which: SpecialColor,
        spec: XColor<'_>,
        via: SpecialVia,
        term: StringTerm,
    ) {
        match spec {
            XColor::Rgb(r, g, b) => self.set_special(which, ColorWord::rgb(r, g, b)),
            XColor::Query => self.report_special(which, via, term),
            XColor::Unresolved(_) => self.unsupported_total += 1,
        }
    }

    /// Answer a special colour's query in the shape it was asked: the
    /// colour set, else the text foreground it draws in (the stored one, an
    /// `OSC 10` override included), silent on a core never told its colours.
    fn report_special(&mut self, which: SpecialColor, via: SpecialVia, term: StringTerm) {
        let own = self.strs.palette[crate::storage::SPECIAL_PALETTE + which.index()];
        let fg = DynamicColor::Foreground.index();
        let word = [own, self.cold.dynamic[fg], self.cold.defaults[fg]]
            .into_iter()
            .find(|w| !w.is_default())
            .unwrap_or(ColorWord::DEFAULT);
        let Some((r, g, b)) = word.as_rgb() else {
            return;
        };
        self.want_reply(Report::SpecialColor {
            which,
            via,
            r,
            g,
            b,
            term,
        });
    }

    /// The same for a dynamic colour. `OSC 12`'s cursor colour is read where
    /// the cursor is drawn, and `OSC 17`'s and `19`'s where a selection is,
    /// and no rendition resolves through them, so they move no epoch.
    pub(crate) fn set_dynamic(&mut self, which: DynamicColor, w: ColorWord) {
        if self.cold.dynamic[which.index()] != w {
            self.cold.dynamic[which.index()] = w;
            if moves_renditions(which) {
                self.style_log.colors_moved();
            }
        }
    }

    fn dispatch_osc(&mut self, term: StringTerm) {
        let n = self.cold.str_len.index();
        // Split the borrow: `Osc::parse` holds the payload while the setters
        // want the other buffers, and an `OSC 4`'s pairs are walked while
        // each is applied. The payload is lent out of `strs` for the match
        // and put back after it; no arm returns early.
        let pending = core::mem::take(&mut self.strs.pending);
        match Osc::parse(&pending[..n]) {
            // **No trailing-NUL trim, because a NUL cannot get here.**
            // `baud-core2` carried a `clip` helper for emitters that put a NUL
            // before the real terminator, and this core had one too until the
            // parser started ignoring C0 in the payload: `graph::string` drops
            // `00-06` outright, so there is no trailing NUL left to trim.
            // Neutering the helper changed no row of the differential, which
            // is a function that cannot fire -- the shape this repo has now
            // recorded seven times.
            Osc::SetTitle(t) => self.set_labels(t, Labels::Title),
            Osc::SetIcon(t) => self.set_labels(t, Labels::Icon),
            Osc::SetIconAndTitle(t) => self.set_labels(t, Labels::Both),
            // Each pair in turn, a query answered where it stands, so a set
            // before it is what it reports.
            Osc::SetColors(pairs) => {
                for pair in pairs {
                    match pair {
                        ColorPair::Entry { index, spec } => match spec {
                            XColor::Rgb(r, g, b) => {
                                self.set_palette(index, ColorWord::rgb(r, g, b));
                            }
                            XColor::Query => self.report_color(index, term),
                            // A spec no reader takes: a name the X server
                            // lacks, or a spec off the grammar or its range
                            // (a name it holds, and every Xcms form in range,
                            // is `Rgb` already, from `XColor::parse`). Left
                            // alone rather than set to something
                            // plausible-but-wrong, and **counted**, because a
                            // spec this core cannot resolve is a request it
                            // did not honour;
                            // `every_spec_osc_code_is_handled_or_counted`
                            // probes each code with `;1`, which for OSC
                            // 10/11/12 is such a spec.
                            XColor::Unresolved(_) => self.unsupported_total += 1,
                        },
                        ColorPair::Special { which, spec } => {
                            self.special_pair(which, spec, SpecialVia::Palette, term)
                        }
                        // A pair off the grammar: a request not honoured.
                        ColorPair::Malformed => self.unsupported_total += 1,
                    }
                }
            }
            Osc::ResetColors(resets) if resets.all() => self.reset_palette(),
            Osc::ResetColors(resets) => {
                for reset in resets {
                    match reset {
                        ColorReset::Entry(c) => self.set_palette(c, ColorWord::DEFAULT),
                        ColorReset::Special(which) => self.set_special(which, ColorWord::DEFAULT),
                        ColorReset::Past | ColorReset::Malformed => self.unsupported_total += 1,
                    }
                }
            }
            // **Under `?5` each names the colour shown in its role** (note V,
            // V17): every cell is drawn exchanged, so a default cell shows the
            // stored background as its text and the stored foreground as its
            // page, and the role maps to the other slot through the [`Video`]
            // the renderer draws by -- where xterm's `OSC 10`-family handlers
            // land, on its swapped tables (`[[source: xterm-misc]]` misc.c
            // 3568-3750). A query keeps the code the program sent, as xterm's
            // replies do.
            // Each spec in turn, from the number sent on: a query answered
            // where it stands.
            //
            // **A cursor colour equal to the text follows a new text
            // colour** when the sequence does not set the cursor's, as
            // xterm's `ChangeColors` moves it (`[[source: xterm-scroll]]`
            // util.c 3062-3070), comparing with the text before the walk.
            Osc::SetDynamic(specs) => {
                let text = self.cold.shown(DynamicColor::Foreground);
                let (mut new_text, mut cursor_set) = (None, false);
                for spec in specs {
                    match spec {
                        DynamicSpec::Color { which, spec } => match spec {
                            XColor::Rgb(r, g, b) => {
                                let slot = self.cold.video().role(which);
                                let w = ColorWord::rgb(r, g, b);
                                self.set_dynamic(slot, w);
                                match which {
                                    DynamicColor::Foreground => new_text = Some(w),
                                    DynamicColor::Cursor => cursor_set = true,
                                    _ => {}
                                }
                            }
                            XColor::Query => self.report_dynamic(which, term),
                            // Counted, as `OSC 4`'s unresolvable name is.
                            XColor::Unresolved(_) => self.unsupported_total += 1,
                        },
                        DynamicSpec::Unheld => self.unsupported_total += 1,
                    }
                }
                let cursor = self.cold.shown(DynamicColor::Cursor);
                if let Some(w) = new_text {
                    if !cursor_set && !cursor.is_default() && cursor == text {
                        self.set_dynamic(DynamicColor::Cursor, w);
                    }
                }
            }
            Osc::ResetDynamic(which) => {
                let slot = self.cold.video().role(which);
                self.set_dynamic(slot, ColorWord::DEFAULT);
            }
            // **Inline, because the URI borrows the payload buffer.** Both
            // live in `strs`, so resolving the slot and the length first is
            // what lets the write happen at all -- the same borrow split
            // `baud-core2` made here for the same reason.
            //
            // **A slot is never overwritten and a clipped URI is never
            // interned**, because a click opens what the slot holds. The
            // table used to evict on collision like the style table, and
            // that is wrong for a link: a later `OSC 8` from *any* program
            // -- an empty run that writes no cell -- landed in the slot an
            // honest link already on screen resolves through, and the screen
            // did not change while its target did. The style table's
            // degradation is a wrong colour; this one is a wrong destination.
            Osc::Hyperlink { id, uri } => {
                let len = uri.len();
                if len == 0 {
                    self.link = LinkId::NONE;
                } else if len > LINK_MAX || n >= OSC_MAX {
                    // A link whose URI may be cut is not a link. The body is
                    // clipped at `OSC_MAX` *before* the URI is split out of
                    // it, so a long `id=` leaves a URI under `LINK_MAX` that
                    // still ends mid-path; and a URI past `LINK_MAX` used to
                    // be clipped here in silence. Either is a different URL
                    // from the one sent. Refused and counted; the run renders
                    // as text.
                    self.link = LinkId::NONE;
                    self.links_clipped += 1;
                } else {
                    let slot = LinkId::bucket(id, uri, NLINK);
                    let at = slot.index() * LINK_MAX;
                    let have = self.strs.link_len[slot.index()].index();
                    if have == 0 {
                        let mut tmp = [0u8; LINK_MAX];
                        tmp[..len].copy_from_slice(uri);
                        self.strs.links[at..at + len].copy_from_slice(&tmp[..len]);
                        self.strs.link_len[slot.index()] = ByteLen::from_index(len);
                        self.link = slot;
                    } else if self.strs.links[at..at + have] == *uri {
                        // The same URI is already here -- resent, or a second
                        // `id` hashing to the same slot -- and one slot for
                        // one target is exactly right.
                        self.link = slot;
                    } else {
                        // Occupied by a different URI: refused, and the cells
                        // that follow are plain text. The `ClusterStore` rule
                        // -- a wrong cluster is not a wrong colour -- and it
                        // bounds the table at 127 distinct targets for the
                        // storage's life, which `links_refused` says when it
                        // is reached. Freed only by `RIS`, which erases every
                        // cell that could name a slot first: freed while one
                        // did, a slot taken by a new link would give that cell
                        // the new target.
                        self.link = LinkId::NONE;
                        self.links_refused += 1;
                    }
                }
                self.refresh_flags();
            }
            // Both counters: `strings_ignored` is what an embedder reads to
            // find OSC codes it might want to implement, and
            // `unsupported_total` is the one the conformance inventory asks.
            // They answer different questions and the second was missing.
            // What the marks are acted on for: the prompt block, tracked so
            // a redraw replaces exactly it, and the zone, which
            // `Term::resize` reads to leave the block's rows un-cut; the
            // prompts and their statuses, which navigation reads; and what
            // the `A` asked of a click (`prompt.rs`).
            // **A prompt mark names a line of the main display**, which the
            // status line is not: from the line it is counted and not acted
            // on (`status.rs`).
            Osc::Semantic(_) if self.on_status() => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
            Osc::Semantic(mark) => {
                self.semantic(mark);
                self.semantic_marks += 1;
            }
            Osc::SetSpecials(pairs) => {
                for pair in pairs {
                    match pair {
                        ColorPair::Special { which, spec } => {
                            self.special_pair(which, spec, SpecialVia::Special, term)
                        }
                        ColorPair::Entry { .. } | ColorPair::Malformed => {
                            self.unsupported_total += 1
                        }
                    }
                }
            }
            Osc::ResetSpecials(resets) if resets.all() => {
                clear_specials(self.strs.palette, self.style_log);
            }
            Osc::ResetSpecials(resets) => {
                for reset in resets {
                    match reset {
                        ColorReset::Special(which) => self.set_special(which, ColorWord::DEFAULT),
                        ColorReset::Entry(_) | ColorReset::Past | ColorReset::Malformed => {
                            self.unsupported_total += 1
                        }
                    }
                }
            }
            // Each `c ; f` in turn, the colour epoch moved when a mode
            // changes, so every rendition is resolved again under it.
            Osc::ColorModes(sets) => {
                for set in sets {
                    match set {
                        ColorModeSet::Mode(m, change) => {
                            let next = self.strs.color_modes.with(m, change);
                            if next != self.strs.color_modes {
                                self.strs.color_modes = next;
                                self.style_log.colors_moved();
                            }
                        }
                        ColorModeSet::Unknown | ColorModeSet::Malformed => {
                            self.unsupported_total += 1
                        }
                    }
                }
            }
            Osc::Allowed(q) => self.report_allowed(q, term),
            // "If Pt is empty, or does not match any of the standard names,
            // xterm uses the resource's default" (`[[source: ctlseqs]]`):
            // the embedder's own pointer here. A name none of the 77 spells
            // is counted, a request not honoured as sent.
            Osc::Pointer(req) => {
                self.cold.pointer = match req {
                    PointerRequest::Shape(shape) => Some(shape),
                    PointerRequest::Default => None,
                    PointerRequest::Unknown => {
                        self.unsupported_total += 1;
                        None
                    }
                };
            }
            Osc::SetProperty(p) => self.set_property(p),
            Osc::LogFile(f) => self.log_file(
                f,
                match n >= OSC_MAX {
                    true => crate::logging::Body::Cut,
                    false => crate::logging::Body::Whole,
                },
            ),
            Osc::Font(req) => self.font_request(req, term),
            Osc::WorkingDirectory(report) => self.report_directory(
                report,
                match n >= OSC_MAX {
                    true => crate::logging::Body::Cut,
                    false => crate::logging::Body::Whole,
                },
            ),
            Osc::Notify9(o) => self.osc9(o),
            Osc::Notify777(o) => self.osc777(o),
            Osc::KittyNotify { meta, payload } => self.notice_whole(meta, payload),
            Osc::Iterm(o) => self.osc1337(o),
            Osc::ColorControl(rest) => self.colour_whole(rest),
            Osc::KittyNotifyMalformed | Osc::Unsupported => {
                self.strings_ignored += 1;
                self.unsupported_total += 1;
            }
        }
        self.strs.pending = pending;
    }

    /// Answer `OSC 60`, `61` or `62` in xterm's shape, with baud's fixed
    /// behaviour written in xterm's model, where a subcategory is allowed
    /// when its category is or when it is not on the category's disallowed
    /// list (`AllowWindowOps` and its siblings, `[[source: xterm-misc]]`):
    /// a category is allowed when baud honours every one of its
    /// subcategories, and a category it does not lists those it refuses.
    /// Decided by Jev, 0.98 both orders, over silence and xterm's defaults.
    /// The paste controls are the embedder's to filter: `OSC 61` names
    /// those it says it replaces, none until it says, and `OSC 60` never
    /// claims the category. A malformed query is counted.
    fn report_allowed(&mut self, q: AllowQuery, term: StringTerm) {
        let facts = self.allow_facts();
        let mut out = AllowList::new(match q {
            AllowQuery::Allowed => b"60",
            AllowQuery::Disallowed(_) => b"61",
            AllowQuery::Allowable(_) => b"62",
            AllowQuery::Malformed => {
                self.unsupported_total += 1;
                return;
            }
        });
        match q {
            AllowQuery::Allowed => {
                for c in AllowCategory::ALL {
                    if allowed(c, facts) {
                        out.item(c.name());
                    }
                }
            }
            AllowQuery::Disallowed(Some(c)) => {
                for s in c.subcategories() {
                    if !honoured(c, s, facts) {
                        out.item(s);
                    }
                }
            }
            AllowQuery::Allowable(Some(c)) => {
                for s in c.subcategories() {
                    out.item(s);
                }
            }
            AllowQuery::Disallowed(None) | AllowQuery::Allowable(None) | AllowQuery::Malformed => {}
        }
        let end: &[u8] = match term {
            StringTerm::Bel => b"\x07",
            StringTerm::St => b"\x1b\\",
        };
        // A truncated reply is a wrong answer (`Paint::want_reply`).
        if out.state == ListState::Cut {
            self.reply_overflows += 1;
            return;
        }
        let len = out.len.index();
        self.push_reply(&[&out.buf[..len], end]);
    }

    /// Whether `OSC 60` lists category `c` now: the one reading XTGETXRES
    /// answers the `allow*` resources from, so the two cannot disagree.
    pub(crate) fn category_allowed(&self, c: AllowCategory) -> bool {
        allowed(c, self.allow_facts())
    }

    /// What baud honours of xterm's feature categories, from the embedder's
    /// settings and what it has told.
    fn allow_facts(&self) -> Facts {
        Facts {
            font: match (self.strs.fonts.known(), self.strs.fonts.changes) {
                (None, _) => Fonts::Untold,
                (Some(_), FontChanges::On) => Fonts::Changeable,
                (Some(_), FontChanges::Off) => Fonts::Fixed,
            },
            checksum: self.cold.checksum_report,
            window_ops: self.strs.window_asks.ops,
            abilities: self.strs.window_asks.abilities,
            window: self.strs.window,
            cell: match self.cell_width != ImagePx::ZERO && self.cell_height != ImagePx::ZERO {
                true => Cell::Known,
                false => Cell::Untold,
            },
            clipboard: match (
                self.strs.clip_area.is_empty(),
                self.strs.window_asks.clipboard,
            ) {
                (false, Osc52Writes::On) => Clipboard::Provisioned,
                _ => Clipboard::Unprovisioned,
            },
            paste: self.strs.window_asks.paste,
        }
    }

    /// Answer `OSC 4 ; c ; ?` in the same shape as the setter.
    ///
    /// `[[source: ctlseqs]]`: xterm "replies with a control sequence of the
    /// same form which can be used to set the corresponding color".
    ///
    /// **Every entry is answered**, as xterm's `ReportAnsiColorRequest` does:
    /// the application's override if it set one, else the embedder's table
    /// ([`crate::Term::set_default_palette`]), else nothing. The last is the
    /// truthful answer of a core no embedder told -- naming a colour the
    /// renderer does not use would be the wrong-shaped answer
    /// `docs/vt-coverage.md` forbids. The table is a second copy of the
    /// renderer's, kept in step by the embedder's one call beside
    /// [`crate::Term::set_default_colors`].
    fn report_color(&mut self, index: Chan, term: StringTerm) {
        let own = self.strs.palette[index.index()];
        let word = if own.is_default() {
            self.strs.palette[crate::storage::EMBEDDER_PALETTE + index.index()]
        } else {
            own
        };
        let Some((r, g, b)) = word.as_rgb() else {
            return;
        };
        self.want_reply(Report::Color {
            index,
            r,
            g,
            b,
            term,
        });
    }
}

impl Paint<'_, '_> {
    /// Answer `OSC 10 ; ?` / `OSC 11 ; ?`: the application's override if it
    /// set one, else the embedder's default if it supplied one, else nothing
    /// -- the `OSC 4` refusal, for the same reason. Of the colour shown in
    /// the role asked, under the code asked (`Cold::shown`): xterm's
    /// `ReportColorRequest` reports "the original color index, but reporting
    /// the opposite color which would be used" (`[[source: xterm-misc]]`
    /// 3576-3580).
    fn report_dynamic(&mut self, which: DynamicColor, term: StringTerm) {
        let Some((r, g, b)) = self.cold.shown(which).as_rgb() else {
            return;
        };
        self.want_reply(Report::DynamicColor {
            which,
            r,
            g,
            b,
            term,
        });
    }
}

/// Whether `OSC 60` lists category `c`: every subcategory honoured, and
/// never the paste controls, which are the embedder's to filter.
fn allowed(c: AllowCategory, facts: Facts) -> bool {
    c != AllowCategory::PasteControls && c.subcategories().iter().all(|s| honoured(c, s, facts))
}

/// Whether baud honours one of xterm's feature subcategories
/// ([`AllowCategory::subcategories`]): what each refuses is in its own row
/// of `docs/vt-coverage.md`. Termcap queries are not built; `OSC 50`'s
/// query is honoured once the embedder has told the font, and its change
/// while the embedder has changes on; of the colours, `SetColor` and `GetColor`, because the pointer
/// colours (`OSC 13`, `14`) and the Tektronix colours (`OSC 15`, `16`,
/// `18`) are refused, and xterm gates them with the rest of `OSC 10` to
/// `19` under those two (`AllocateTermColor` and `ReportColorRequest`,
/// `[[source: xterm-misc]]` 6456 and 3572); of the window, the
/// manipulations and the line count, the title reports, `OSC 52`'s read,
/// `OSC 3` and DECRQCRA while the embedder has each off, and `OSC 52`'s
/// write on a storage with no clipboard area, which refuses every write.
/// XTCHECKSUM, `SetChecksum`, is honoured whatever the checksum setting
/// (`Paint::select_checksum`).
///
/// **A manipulation is honoured while the operations are on and the
/// embedder carries out every form of it** ([`WindowAbilities`]). **A
/// report is honoured when it answers**, each from what the embedder has
/// told, by xterm's gates (`[[source: xterm-reset]]`'s file, `window_ops`): `GetWinState`
/// `CSI 11 t`, `GetWinPosition` `CSI 13 t` and `CSI 13 ; 2 t`,
/// `GetWinSizePixels` `CSI 14 t` and `CSI 14 ; 2 t`, `GetScreenSizeChars`
/// `CSI 16 t` and `CSI 19 t`; `GetWinSizeChars`, `CSI 18 t`, always.
fn honoured(c: AllowCategory, sub: &[u8], facts: Facts) -> bool {
    match c {
        AllowCategory::ColorOps => sub == b"GetAnsiColor",
        // **The paste controls the embedder replaces**
        // ([`crate::Term::set_paste_controls`]), none until it says.
        AllowCategory::PasteControls => !facts.paste.disallows(sub),
        AllowCategory::TitleOps => true,
        AllowCategory::FontOps => match sub {
            b"GetFont" => facts.font != Fonts::Untold,
            b"SetFont" => facts.font == Fonts::Changeable,
            _ => false,
        },
        AllowCategory::TcapOps => false,
        AllowCategory::MouseOps => true,
        AllowCategory::WindowOps => match sub {
            _ if let Some(m) = WindowManipulation::of_name(sub) => {
                facts.window_ops == WindowOps::On && facts.abilities.has(m)
            }
            b"GetWinState" => facts.window.state.is_some(),
            b"GetWinPosition" => {
                facts.window.position.is_some() && facts.window.text_area.is_some()
            }
            b"GetWinSizePixels" => facts.cell == Cell::Known && facts.window.window.is_some(),
            b"GetScreenSizeChars" => facts.cell == Cell::Known && facts.window.screen.is_some(),
            b"GetWinSizeChars" | b"PushTitle" | b"PopTitle" | b"ColumnMode" | b"SetChecksum" => {
                true
            }
            b"GetChecksum" => facts.checksum == ChecksumReport::On,
            b"SetXprop" => facts.window_ops == WindowOps::On,
            // **DECSSDT and DECSASD, whatever the operations say**: with them
            // off the window keeps its size and the grid gives the line a
            // row (`status.rs`), where xterm refuses both under
            // `AllowWindowOps(xw, ewStatusLine)`.
            b"StatusLine" => true,
            b"SetSelection" => facts.clipboard == Clipboard::Provisioned,
            _ => false,
        },
    }
}

/// What the feature replies read of the terminal's own setup.
#[derive(Clone, Copy)]
struct Facts {
    font: Fonts,
    checksum: ChecksumReport,
    window_ops: WindowOps,
    abilities: WindowAbilities,
    window: WindowFacts,
    cell: Cell,
    clipboard: Clipboard,
    paste: PasteControls,
}

/// Whether the embedder has told the cell's size (`Term::set_cell_size`),
/// which the pixel reports divide or multiply by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Known,
    Untold,
}

/// What `OSC 50` can do here: nothing with no font told
/// (`Term::set_font`), query and change, or query alone while the embedder
/// has changes off.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fonts {
    Untold,
    Changeable,
    Fixed,
}

/// Whether an `OSC 52` write reaches the clipboard: the storage has an
/// area for it ([`crate::Owned::with_clipboard`]) and the embedder takes
/// it ([`Osc52Writes`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clipboard {
    Provisioned,
    Unprovisioned,
}

/// An `OSC 60`-family reply as it is built: `ESC ] Ps`, then `;` and the
/// items separated by `,`. Its buffer holds the longest, `OSC 62 ;
/// allowWindowOps`, which `the_longest_feature_reply_fits` measures.
struct AllowList {
    buf: [u8; AllowList::CAP],
    len: ByteLen,
    state: ListState,
}

/// Where an [`AllowList`] is: no item yet, items, or cut short by its
/// buffer -- a wrong answer, never sent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListState {
    Empty,
    Items,
    Cut,
}

impl AllowList {
    const CAP: usize = 384;

    fn new(ps: &[u8]) -> AllowList {
        let mut l = AllowList {
            buf: [0; AllowList::CAP],
            len: ByteLen::ZERO,
            state: ListState::Empty,
        };
        l.put(b"\x1b]");
        l.put(ps);
        l
    }

    fn put(&mut self, b: &[u8]) {
        let at = self.len.index();
        match self.buf.get_mut(at..at + b.len()) {
            Some(to) => {
                to.copy_from_slice(b);
                self.len = ByteLen::from_index(at + b.len());
            }
            None => self.state = ListState::Cut,
        }
    }

    fn item(&mut self, name: &[u8]) {
        match self.state {
            ListState::Empty => {
                self.put(b";");
                if self.state == ListState::Empty {
                    self.state = ListState::Items;
                }
            }
            ListState::Items => self.put(b","),
            ListState::Cut => return,
        }
        self.put(name);
    }
}

/// Every `OSC 4` override cleared -- the first [`EMBEDDER_PALETTE`] entries
/// of the palette slice -- with the colour epoch moved once if any entry
/// changed. The one spelling of the clear, for `OSC 104`, both resets and an
/// embedder's [`Overrides::Drop`](crate::Overrides::Drop).
///
/// [`EMBEDDER_PALETTE`]: crate::storage::EMBEDDER_PALETTE
pub(crate) fn clear_palette(palette: &mut [ColorWord], log: &mut StyleLog<'_>) {
    let mut moved = false;
    for w in &mut palette[..crate::storage::EMBEDDER_PALETTE] {
        if !w.is_default() {
            *w = ColorWord::DEFAULT;
            moved = true;
        }
    }
    if moved {
        log.colors_moved();
    }
}

/// Every special colour cleared, the colour epoch moved if one was set:
/// both resets, `OSC 104` and `OSC 105` with no parameter, and an
/// embedder's `Overrides::Drop`, as xterm's reset of all `MAXCOLORS` takes
/// them with the palette (`[[source: xterm-misc]]` 3070-3080).
pub(crate) fn clear_specials(palette: &mut [ColorWord], log: &mut StyleLog<'_>) {
    let special = &mut palette[crate::storage::SPECIAL_PALETTE..crate::storage::SPECIAL_END];
    if special.iter().any(|w| !w.is_default()) {
        special.fill(ColorWord::DEFAULT);
        log.colors_moved();
    }
}

/// Whether renditions resolve through a dynamic colour: the default
/// foreground and background, and not the cursor's or the selection's.
fn moves_renditions(which: DynamicColor) -> bool {
    matches!(which, DynamicColor::Foreground | DynamicColor::Background)
}

/// Every `OSC 10`/`11`/`12`/`17`/`19` override cleared, with the colour epoch
/// moved when the foreground or background changed -- the cursor's and the
/// selection's colours move none, as in `set_dynamic`.
pub(crate) fn clear_dynamic(cold: &mut Cold, log: &mut StyleLog<'_>) {
    for which in [
        DynamicColor::Foreground,
        DynamicColor::Background,
        DynamicColor::Cursor,
        DynamicColor::HighlightBg,
        DynamicColor::HighlightFg,
    ] {
        let w = &mut cold.dynamic[which.index()];
        if !w.is_default() {
            *w = ColorWord::DEFAULT;
            if moves_renditions(which) {
                log.colors_moved();
            }
        }
    }
}

/// Which names an `OSC 0`, `1` or `2` sets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Labels {
    Title,
    Icon,
    Both,
}

fn copy_bounded(dst: &mut [u8], src: &[u8]) -> ByteLen {
    let n = src.len().min(OSC_MAX);
    dst[..n].copy_from_slice(&src[..n]);
    ByteLen::from_index(n)
}

/// Which control string is being accumulated.
///
/// How much of a DECRSPS body in DECCIR's format is read: the longest data
/// string with room for a byte past it, which makes it invalid.
const CIR_COPY: usize = CIR_DATA_MAX + 1;

/// Five states for five introducers. `SOS`, `PM` and `APC` share `Other`,
/// with any `DCS` this core has no prefix parser for, because it acts on none
/// of them and their bodies are consumed identically. `Decrqss` and `Sixel`
/// are separate because each is reached through a **prefix** -- `$ q`, or a
/// bare `q`, after the introducer and any parameters -- rather than through
/// the introducer alone, so `StrKind::of` cannot name them; `Act::DcsQuery`
/// and `Act::SixelStart` set them when the prefix arrives. `Clipboard` is the
/// same again inside an OSC body, set by `str_put` at `52;`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StrKind {
    Osc,
    /// A `DCS $ q`, the status request `report_status` answers.
    Decrqss,
    /// A `DCS Pa;Pb;Ph q` Sixel: the body is raster data, decoded as it
    /// arrives and recorded at the terminator. Reached through the bare `q`
    /// prefix, the `Decrqss` shape one prefix over.
    Sixel,
    /// An `OSC 52` past its `52;`: the body goes to the clipboard areas,
    /// decoded as it arrives (`clip.rs`). Reached through the body's own
    /// first bytes, which `StrKind::of` cannot see either.
    Clipboard,
    Other,
    /// An `APC` before its first body byte, which decides whether it is a
    /// kitty graphics command (`G`) or `Other`.
    Apc,
    /// An `APC G`: the body goes to the kitty graphics stream (`kitty.rs`).
    Kitty,
    /// A `DCS 1 $ t`, DECRSPS in DECCIR's format: the body is kept and
    /// restored at the terminator (`Paint::restore_cursor_information`).
    RestoreCursor,
    /// A `DCS 2 $ t`, DECRSPS in DECTABSR's format: each stop is set as its
    /// column arrives (`Paint::restore_tab_put`), so no length limits it.
    RestoreTabs,
    /// A `DCS Pn p`, ReGIS: each body byte goes to the interpreter as it
    /// arrives (`crate::regis`), and nothing is kept.
    Regis,
    /// A `DCS Pc $ w`, DECLKD: the body is kept and loaded at the
    /// terminator (`Paint::locator_keys_end`).
    LocatorKeys,
    /// A `DCS +`, with no parameter before it: its body, a `q` or a `p` and
    /// then the hex list, is kept as a DECRQSS body is and read at the
    /// terminator (`Paint::tcap_end`), XTGETTCAP or XTSETTCAP.
    Tcap,
    /// A `Tcap` body cut at `OSC_MAX`: the name the cut fell in is not the
    /// name that was sent, so it is refused (`Paint::tcap_end`).
    TcapCut,
    /// An `OSC 99` past its metadata: the payload goes to the
    /// notifications' chunk, decoded as it arrives (`notice.rs`). Reached
    /// through the body's own bytes, as `Clipboard` is.
    Notice,
    /// A `DCS 1 $ p`, DECRSTS of a terminal state report: the body is kept
    /// and checksummed at the terminator, then restored
    /// (`Paint::restore_terminal_state`).
    RestoreTerminal,
    /// A `DCS 2 $ p`, DECRSTS of a colour table report: each group is set
    /// as its `/` arrives (`Paint::restore_color_put`), so no length limits
    /// it.
    RestoreColors,
    /// An `OSC 21` past its `21;`: kitty's colour control, each record read
    /// at its `;` (`colour.rs`). Reached through the body's own bytes, as
    /// `Clipboard` is.
    ColorControl,
}

impl StrKind {
    pub(crate) const fn of(b: StreamByte) -> StrKind {
        match b.raw_byte() {
            // **Both spellings of the same introducer.** `ESC ]` is the
            // seven-bit form and `9D` the eight-bit one, and the graph reaches
            // the same node from either -- a Latin-1 `9D` is routed as
            // `escape(9D - 0x40)`, which is `]`. But this reads the byte back
            // out of the arena to name the kind, so it saw `9D` and answered
            // `Other`: the string parsed, its body was consumed, and the title
            // was silently dropped.
            //
            // The graph's delegation is what makes one row serve both, and
            // this is the one place downstream that still had to be told.
            //
            // **And `DD`, the same `]` in GR under ISO 8859-1**, where the
            // escape state reads a GR byte as its 7-bit byte (`graph::one`,
            // `[[source: williams]]` 462): `ESC DD` is `ESC ]`, and this read
            // `Other` for it, consuming the string and dropping its title.
            // Under UTF-8 the graph names `StrStart` for no `DD`.
            0x5d | 0x9d | 0xdd => StrKind::Osc,
            // `APC`, in the same three spellings: `ESC _`, `9F` and `DF`.
            0x5f | 0x9f | 0xdf => StrKind::Apc,
            _ => StrKind::Other,
        }
    }
}
