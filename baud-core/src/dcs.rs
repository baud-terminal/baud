//! DECRQSS: the one DCS either core acts on.
//!
//! `[[source: ctlseqs]]` gives `DCS $ q Pt ST` as Request Status String, where
//! `Pt` names a setting by the tail of the CSI that would set it -- `m` for
//! SGR, `r` for DECSTBM, `"q"` for DECSCA. The reply is the *tail of the CSI
//! that would set it*, so an application can echo it straight back.
//!
//! Every other DCS -- Sixel, DECUDK, DECRQUPSS -- is consumed to its
//! terminator and ignored, which is the half that matters: a DCS body reaching
//! the screen would render an application's private protocol as glyphs.

use crate::paint::Paint;
use crate::storage::REPLY;
use baud_domain::*;

impl Paint<'_, '_> {
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
    #[cold]
    #[inline(never)]
    pub(crate) fn report_status(&mut self, t: StatusTarget) {
        // **A setting this core does not record answers `0$r`, not silence.**
        // `docs/vt-coverage.md`'s rule is never to answer a query with the
        // wrong shape, and "not recognised" is a shape DECRQSS has. Silence
        // would leave an application waiting for a terminator that never
        // arrives, which is worse than either answer.
        // **A setting this core does not record is `0$r` too.** A target that
        // fell through to the `1$r` prefix and then matched no arm would emit
        // `DCS 1 $ r ST`: a "valid" flag over an **empty body**, which an
        // application reads as "the margins are unset" rather than as "I do
        // not implement that". `DECSCUSR` was here until the cursor style was
        // recorded and `DECSLRM` until the margins were, and both left the
        // same way: the list shrinks when a feature lands, and the pin in
        // `every_fixture_owes_the_same_reply` goes red to say so: the DECRQSS
        // fixtures are in the golden capture with the replies core2 gave.
        let unanswerable = matches!(t, StatusTarget::Unknown);
        if unanswerable {
            self.dcs_open();
            self.emit(b"0$r");
            self.dcs_close();
            return;
        }
        self.dcs_open();
        self.emit(b"1$r");
        match t {
            StatusTarget::Sgr => {
                let raw = self.style_raw;
                self.emit_sgr(raw);
                self.emit(b"m");
            }
            StatusTarget::ScrollRegion => {
                self.emit_num(self.cold.top.one_based());
                self.emit(b";");
                self.emit_num(self.cold.bot.one_based());
                self.emit(b"r");
            }
            StatusTarget::CursorStyle => {
                self.emit_dec(WireByte::of(self.cold.cursor_style.wire()));
                self.emit(b" q");
            }
            StatusTarget::Protection => {
                self.emit_dec(WireByte::of(u8::from(self.cold.protect.is_protected())));
                self.emit(b"\"q");
            }
            StatusTarget::SideMargins => {
                self.emit_num(self.cold.left.one_based());
                self.emit(b";");
                self.emit_num(self.cold.right.one_based());
                self.emit(b"s");
            }
            // Unreachable: the guard above answers it with `0$r`. Spelled out
            // rather than `_ =>` so that adding a `StatusTarget` stops the
            // build here instead of silently joining them.
            StatusTarget::Unknown => {}
        }
        self.dcs_close();
    }

    /// The `DCS` introducer, seven-bit or eight, as `S7C1T`/`S8C1T` last said.
    ///
    /// **A `DCS` reply is a C1 control like any other and this path wrote the
    /// bytes literally**, so `S8C1T` moved every `CSI` answer to the eight-bit
    /// form and left `DECRQSS` announcing itself with `ESC P`. An application
    /// that asked for eight-bit replies and parses them as such reads the
    /// `ESC` as the start of something else.
    ///
    /// `[[source: ctlseqs]]`: `DCS` is `90` and `ST` is `9C`.
    fn dcs_open(&mut self) {
        match self.cold.c1 {
            C1Transmit::SevenBit => self.emit(b"\x1bP"),
            C1Transmit::EightBit => self.emit(b"\x90"),
        }
    }

    fn dcs_close(&mut self) {
        match self.cold.c1 {
            C1Transmit::SevenBit => self.emit(b"\x1b\\"),
            C1Transmit::EightBit => self.emit(b"\x9c"),
        }
    }

    /// Append to the pending reply, or count the loss.
    ///
    /// **A truncated reply is a wrong answer**, so an overflow drops what did
    /// not fit and counts it rather than sending half a sequence. The same
    /// rule `want_reply` follows, and the reason `Report::MAX_LEN` is pinned
    /// against every variant.
    fn emit(&mut self, bytes: &[u8]) {
        let at = self.reply_len.index();
        if at + bytes.len() > REPLY {
            self.reply_overflows += 1;
            return;
        }
        self.reply[at..at + bytes.len()].copy_from_slice(bytes);
        self.reply_len = ByteLen::from_index(at + bytes.len());
    }

    /// The current rendition as an SGR parameter string, always starting at 0.
    ///
    /// **The leading `0` is not padding.** DECRQSS answers with something the
    /// application can send back verbatim, and a string that only *adds*
    /// attributes would not reproduce this state on a terminal that had others
    /// set.
    fn emit_sgr(&mut self, w: StyleWord) {
        self.emit(b"0");
        for (bit, code) in Attrs::WIRE {
            if w.attrs().has(bit) {
                self.emit(b";");
                self.emit_dec(WireByte::of(code));
            }
        }
        // `SGR 10` is the primary font and is the default, so it is emitted
        // only when it is not -- a redundant `10` is noise a comparison
        // against the default would fail on.
        if !w.font().is_primary() {
            self.emit(b";");
            self.emit_dec(WireByte::of(w.font().sgr()));
        }
        if !w.underline().is_default() {
            self.emit(b";");
            self.emit_colour(w.underline(), WireByte::of(58));
        }
        let (fg, bg) = (w.fg(), w.bg());
        if !fg.is_default() {
            self.emit(b";");
            self.emit_colour(fg, WireByte::of(38));
        }
        if !bg.is_default() {
            self.emit(b";");
            self.emit_colour(bg, WireByte::of(48));
        }
    }

    fn emit_colour(&mut self, c: ColorWord, base: WireByte) {
        if let Some((r, g, b)) = c.as_rgb() {
            self.emit_dec(base);
            self.emit(b";2;");
            self.emit_dec(WireByte::of(r));
            self.emit(b";");
            self.emit_dec(WireByte::of(g));
            self.emit(b";");
            self.emit_dec(WireByte::of(b));
        } else if let Some(i) = c.as_index() {
            self.emit_dec(base);
            self.emit(b";5;");
            self.emit_dec(WireByte::of(i));
        }
    }

    /// A 1-based coordinate as decimal. Wider than `emit_dec`, because a
    /// margin can be past column 255.
    fn emit_num(&mut self, v: OneBased) {
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
        self.emit(&tail[i..]);
    }

    /// A wire code as decimal, at most three digits.
    ///
    /// Takes a [`WireByte`] rather than a `u8` so it cannot be handed a
    /// coordinate: a right margin can sit past column 255 and this saturates
    /// there, writing the wrong number rather than failing. `emit_num` is the
    /// one for those, and the types are what keep them apart.
    fn emit_dec(&mut self, v: WireByte) {
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
        self.emit(&tail[i..]);
    }
}
