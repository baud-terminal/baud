use crate::*;

impl<'a> Term<'a> {
    /// Act on a completed DCS payload.
    pub(crate) fn dispatch_dcs(&mut self) {
        let n = self.osc_len.index();
        let handled = self.dcs_final.map(|f| Dcs::parse(self.csi_inter, f));
        if handled == Some(Dcs::RequestStatus) {
            let target = StatusTarget::parse(&self.s.osc[..n]);
            self.report_status(target);
        }
        self.osc_reset();
        self.dcs_final = None;
    }

    /// Answer a DECRQSS.
    ///
    /// `[[source: ctlseqs]]`: "xterm responds with `DCS 1 $ r Pt ST` for valid
    /// requests, replacing the Pt with the corresponding CSI string, or
    /// `DCS 0 $ r ST` for invalid requests."
    ///
    /// **1 is valid and 0 is invalid, and the DEC manuals say the opposite.**
    /// `[[source: ctlseqs]]` records the errata directly: "testing a DEC VT420
    /// in 1996 showed that the documented code for a valid or invalid response
    /// to DECRQSS was reversed", and that vttest and DEC STD 070 agree with the
    /// tested behaviour rather than with the VT420 and VT525 manuals. baud
    /// follows xterm, which is what applications are written against. Getting
    /// this backwards would tell every application that every setting it asked
    /// about is unsupported.
    ///
    /// The reply body is the *tail of the CSI that would set it*, so an
    /// application can echo it straight back.
    pub(crate) fn report_status(&mut self, t: StatusTarget) {
        // One place where the DECRQSS shape is written, for the same reason
        // replies go through `Report`: the introducer, the validity digit and
        // the terminator must not be spelled at each call site. The body is
        // variable-length, which is why this is a helper rather than a
        // `Report` variant over the fixed-size encoder.
        if t == StatusTarget::Unknown {
            let (dcs, st) = (self.c1.dcs(), self.c1.st());
            self.emit(dcs);
            self.emit(b"0$r");
            self.emit(st);
            return;
        }
        let dcs = self.c1.dcs();
        self.emit(dcs);
        self.emit(b"1$r");
        match t {
            StatusTarget::ScrollRegion => {
                self.emit_num(self.scroll_top.one_based());
                self.emit(b";");
                self.emit_num(self.scroll_bot.one_based());
                self.emit(b"r");
            }
            StatusTarget::SideMargins => {
                self.emit_num(self.left_margin.one_based());
                self.emit(b";");
                self.emit_num(self.right_margin.one_based());
                self.emit(b"s");
            }
            StatusTarget::CursorStyle => {
                self.emit_dec(self.cursor_style.wire());
                self.emit(b" q");
            }
            StatusTarget::Protection => {
                self.emit_dec(u8::from(self.protect.is_protected()));
                self.emit(b"\"q");
            }
            StatusTarget::Sgr => {
                let raw = self.style_raw;
                self.emit_sgr(raw);
                self.emit(b"m");
            }
            StatusTarget::Unknown => {}
        }
        let st = self.c1.st();
        self.emit(st);
    }
}
