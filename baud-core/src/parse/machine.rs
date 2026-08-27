use crate::*;

impl<'a> Term<'a> {
    pub(crate) fn dispatch(&mut self, f: FinalByte) {
        let n = self.nparam.count_at_least_one();
        let marker = self.private;
        let p = Params::with_subs(&self.params[..n], self.psub);
        let lrm = match self.modes.is_set(ModeBit::SideMargins) {
            true => MarginMode::On,
            false => MarginMode::Off,
        };
        match Csi::parse_with(marker, self.csi_inter, p, f, lrm) {
            Csi::SoftReset => self.soft_reset(),
            // `CSI ? 1 ; 3 ; 4 ; 6 l` sets *four* modes, and rxvt's own reset
            // string is exactly that. Reading only the first parameter left the
            // rest silently unset -- `CSI ? 7 ; 25 l` reset autowrap and left
            // the cursor visible. Parameters are copied out first because
            // `set_private_mode` takes `&mut self`.
            Csi::PrivateMode { change, .. } => self.apply_private_modes(change),
            // `[[source: vt510-decsca]]`: "Ps defines all characters that
            // follow the DECSCA function as erasable or not erasable". It does
            // not repaint anything already on screen, and it does not touch the
            // rendition -- hence a flag, not an SGR fold.
            Csi::SelectProtection { protect } => {
                self.protect = CellFlags::protection(protect.is_protected());
                self.refresh_flags();
            }
            Csi::CursorStyle(c) => self.cursor_style = c,
            Csi::RequestMode { marker, mode } => {
                let state = self.mode_state(marker, mode);
                self.report(Report::ModeStatus {
                    marker,
                    mode,
                    state,
                });
            }
            Csi::SelectiveEraseDisplay(e) => self.selective_erase_display(e),
            Csi::SelectiveEraseLine(e) => self.selective_erase_line(e),
            Csi::Repeat(c) => {
                if let Some((cp, w)) = self.last_cp {
                    // Bounded by the row: a repeat cannot usefully exceed the
                    // line it is filling, and `CSI 65535 b` should not spin.
                    let k = c.at_least_one().min(self.cols.index());
                    for _ in 0..k {
                        self.put_cp(cp, w);
                    }
                }
            }
            // `[[source: ctlseqs]]` lists the Primary DA identities. VT102 is
            // what baud actually is: it has DECSTBM (VT100) and IL/DL (VT102),
            // and it does NOT have sixel, ReGIS, UDKs, NRCS or 132-column mode.
            // Selective erase is a VT220 feature and has since landed, but a DA
            // identity promises everything at that level, so the answer does
            // not move. Under-claiming degrades, over-claiming breaks.
            Csi::RequestVersion => self.report(Report::Version),
            Csi::SetSideMargins { left, right } => self.set_side_margins(left, right),
            Csi::InsertColumns(n) => self.insert_columns(n),
            Csi::DeleteColumns(n) => self.delete_columns(n),
            Csi::DeviceAttributes => {
                self.report(Report::PrimaryDeviceAttributes(DeviceAttribute::Vt102))
            }
            Csi::DeviceStatus(Some(baud_domain::StatusKind::Operating)) => {
                self.report(Report::OperatingStatus)
            }
            Csi::DeviceStatus(Some(baud_domain::StatusKind::CursorPosition)) => {
                // 1-based like CUP, and through `cursor()` rather than off
                // `self.cx` -- the deferred-wrap sentinel is not a column.
                let (cy, cx) = self.cursor();
                self.report(Report::CursorPosition {
                    row: cy.one_based(),
                    col: cx.one_based(),
                });
            }
            // A DSR baud does not implement gets no answer. Replying with the
            // wrong report shape is worse than silence: the application parses
            // it as the report it asked for.
            Csi::DeviceStatus(None) => {}
            Csi::CursorPosition { row, col } => {
                let base = self.origin_row().index();
                let want = Row::from_index(base + row.to_row(self.rows).index());
                self.cy = self.confine(want);
                self.cx = col.to_col(self.cols);
            }
            Csi::CursorUp(c) => self.cy = self.confine(self.cy.up(c)),
            Csi::CursorDown(c) => self.cy = self.confine(self.cy.down(c, self.rows)),
            Csi::CursorForward(c) => self.cx = self.cx.forward(c, self.cols),
            Csi::CursorBack(c) => self.cx = self.cx.back(c),
            Csi::EraseInLine(e) => self.erase_line(e),
            Csi::EraseInDisplay(e) => self.erase_display(e),
            Csi::EraseChars(c) => self.erase_chars(c),
            Csi::DeleteChars(c) => self.delete_chars(c),
            Csi::InsertBlanks(c) => self.insert_blanks(c),
            Csi::CursorForwardTab(c) => self.tab_forward(c),
            Csi::CursorBackTab(c) => self.tab_back(c),
            Csi::TabClear(Some(baud_domain::TabClear::AtCursor)) => {
                let x = self.cx.index();
                self.s.tabs[x] = false;
            }
            Csi::TabClear(Some(baud_domain::TabClear::All)) => {
                for stop in self.s.tabs.iter_mut() {
                    *stop = false;
                }
            }
            Csi::TabClear(None) => {}
            Csi::CursorNextLine(c) => {
                self.cy = self.confine(self.cy.down(c, self.rows));
                self.cx = Col::ZERO;
            }
            Csi::CursorPrevLine(c) => {
                self.cy = self.confine(self.cy.up(c));
                self.cx = Col::ZERO;
            }
            Csi::ColumnAbsolute(c) => self.cx = c.to_col(self.cols),
            Csi::LineAbsolute(r) => {
                let base = self.origin_row().index();
                let want = Row::from_index(base + r.to_row(self.rows).index());
                self.cy = self.confine(want);
            }
            Csi::ColumnRelative(c) => self.cx = self.cx.forward(c, self.cols),
            Csi::LineRelative(c) => self.cy = self.confine(self.cy.down(c, self.rows)),
            Csi::SaveCursor => {
                self.saved = Some((self.cy, self.cx, self.style, self.style_raw, self.flags))
            }
            Csi::RestoreCursor => {
                // `[[source: vt510-decsc]]` lists the selective erase
                // attribute among what DECSC saves, so it is restored here with
                // the rendition rather than left behind.
                if let Some((r, c, st, sw, fl)) = self.saved {
                    self.cy = r;
                    self.cx = c;
                    self.style = st;
                    self.style_raw = sw;
                    self.flags = fl;
                }
            }
            Csi::InsertLines(c) => self.insert_lines(c),
            Csi::DeleteLines(c) => self.delete_lines(c),
            Csi::ScrollUp(c) => self.scroll_region_up(c),
            Csi::ScrollDown(c) => self.scroll_region_down(c),
            Csi::SetScrollRegion { top, bottom } => self.set_scroll_region(top, bottom),
            Csi::Mode { change, .. } => self.apply_ansi_modes(change),
            Csi::Sgr => self.apply_sgr(),
            // Recognised as a CSI and not acted on -- countable, not silent.
            //
            // No `_` arm: every variant is now named, so adding one to `Csi`
            // without handling it here is a compile error rather than a silent
            // drop. That is the `parsed` failure prevented structurally.
            Csi::Unsupported { .. } => self.unsupported.inc(),
        }
    }

    /// The C0 `execute` action.
    ///
    /// Separate from the Ground branch because `[[source: williams]]` has the
    /// escape and CSI states execute C0 controls mid-sequence rather than
    /// treating them as the end of the sequence: `event 00-17,19,1C-1F /
    /// execute`. A stream carrying `ESC [ 1 ; \n 2 m` gets its line feed *and*
    /// its SGR.
    #[inline(always)]
    pub(crate) fn execute(&mut self, b: StreamByte) {
        if b == StreamByte::LF {
            self.line_feed();
        } else if b == StreamByte::CR {
            self.cx = Col::ZERO;
        } else if b == StreamByte::BS {
            self.cx = self.cx.back(Count::ONE);
        } else if b == StreamByte::TAB {
            self.tab_forward(Count::ONE);
        } else if b == StreamByte::VT || b == StreamByte::FF {
            // Both are an index. A terminal does not eject a page, and they were
            // being dropped, so a stream containing VT lost the line break.
            self.index();
        } else if b == StreamByte::SO {
            // `[[source: ctlseqs]]`: "Switch to Alternate Character Set (Ctrl-N
            // is Shift Out or LS1)."
            self.gl = GSlot::G1;
            self.refresh_flags();
        } else if b == StreamByte::SI {
            self.gl = GSlot::G0;
            self.refresh_flags();
        } else if b == StreamByte::BEL {
            // No UI here to ring one, so count it. A core that silently drops
            // BEL leaves an embedder no way to implement one.
            self.bells.inc();
        }
        // Anything else in C0 is discarded, never printed.
    }

    #[inline(always)]
    pub(crate) fn control(&mut self, b: StreamByte) {
        match self.st {
            State::Ground | State::Vt52Ground => {
                if b == StreamByte::ESC {
                    self.st = State::Escape;
                } else if b.is_c0() || b == StreamByte::BEL {
                    self.execute(b);
                } else if let Some((rem, acc)) = b.utf8_lead() {
                    self.st = State::Utf8 {
                        remaining: Pending::new(rem),
                        acc: Utf8Acc::lead(acc),
                    };
                } else {
                    // stray continuation byte or 0x7f: not text
                    self.replacements.inc();
                    self.put_cp(Codepoint::REPLACEMENT, Width::NARROW);
                }
            }
            State::Utf8 { remaining, acc } => match b.utf8_continuation() {
                Some(bits) => {
                    let acc = acc.push(bits);
                    if remaining.is_last() {
                        self.st = self.ground();
                        let c = acc.finish();
                        self.put_cp(c, Width::of(c));
                    } else {
                        self.st = State::Utf8 {
                            remaining: remaining.dec(),
                            acc,
                        };
                    }
                }
                None => {
                    self.st = self.ground();
                    self.replacements.inc();
                    self.put_cp(Codepoint::REPLACEMENT, Width::NARROW);
                    self.control(b);
                }
            },
            State::Escape => {
                // In VT52 mode the escape table is a different protocol, not a
                // subset of this one: `ESC D` is cursor-left there and IND
                // here.
                //
                // **Strictly VT52, including `ESC [`.** A VT52 has no CSI, so
                // allowing one here would be inventing a hybrid terminal that
                // never existed and that no application targets. The cost is
                // real and worth stating: the *only* ways out are `ESC <` and
                // a host-side reset, so a program that enters VT52 and dies
                // leaves the terminal there. That is how the hardware behaved
                // and how xterm behaves; `reset(1)` sending `ESC c` does not
                // help, because `ESC c` is not a VT52 sequence either.
                if self.terminal_mode == TerminalMode::Vt52 {
                    self.vt52_dispatch(b);
                    if self.st == State::Escape {
                        self.st = self.ground();
                    }
                    return;
                }
                if b == StreamByte::CSI_INTRO {
                    self.params = [None; 16];
                    self.nparam = ParamIdx::FIRST;
                    self.psub = SubParams::NONE;
                    self.private = Marker::Standard;
                    self.csi_inter = None;
                    self.st = State::CsiEntry;
                } else if b == StreamByte::OSC_INTRO {
                    self.st = State::OscString;
                } else if b == StreamByte::DCS_INTRO {
                    self.params = [None; 16];
                    self.nparam = ParamIdx::FIRST;
                    self.psub = SubParams::NONE;
                    self.private = Marker::Standard;
                    self.csi_inter = None;
                    self.osc_reset();
                    self.st = State::DcsEntry;
                } else if b == StreamByte::SOS_INTRO
                    || b == StreamByte::PM_INTRO
                    || b == StreamByte::APC_INTRO
                {
                    // Consume the payload; never execute it. These fell through
                    // to `esc_fe` and back to Ground, so the bytes that followed
                    // were interpreted as commands.
                    self.st = State::StringIgnore;
                } else if b.is_esc_intermediate() {
                    self.esc_inter = Some(b);
                    self.st = State::EscapeIntermediate;
                } else {
                    // Same two-byte escapes the AVX2 path handles. The two feed
                    // paths must be indistinguishable; `feed_matches_feed_avx2`
                    // in the property suite asserts it.
                    self.esc_fe(b);
                    self.st = self.ground();
                }
            }
            State::EscapeIntermediate => {
                if b.is_c0() {
                    self.execute(b);
                } else if b.is_esc_intermediate() {
                    self.esc_inter = Some(b);
                } else if b != StreamByte::DEL {
                    match self.esc_inter {
                        // `ESC # 8` is DECALN.
                        Some(i) if i == StreamByte::HASH && b.raw_byte() == b'8' => {
                            self.screen_alignment()
                        }
                        // `ESC ( ) * + <C>` designate G0-G3.
                        Some(i) => {
                            if let Some(slot) = GSlot::from_intermediate(i) {
                                self.designate(slot, b);
                            }
                        }
                        None => {}
                    }
                    self.st = self.ground();
                }
            }
            // `[[source: williams]]`'s dcs_entry / dcs_param / dcs_intermediate.
            // Collecting the sequence's identity; the final byte decides
            // whether the data that follows is collected or discarded.
            State::DcsEntry | State::DcsParam | State::DcsIntermediate => {
                if let Some(d) = b.as_ascii_digit() {
                    if self.nparam.is_first() {
                        self.nparam = self.nparam.next();
                    }
                    let cur = self.params[self.nparam.raw_index() - 1].unwrap_or(Param::ZERO);
                    self.params[self.nparam.raw_index() - 1] = Some(cur.push(d));
                    self.st = State::DcsParam;
                } else if b == StreamByte::SEMICOLON {
                    if self.nparam.is_first() {
                        self.nparam = self.nparam.next();
                    }
                    if self.nparam.raw_index() < 16 {
                        self.params[self.nparam.raw_index()] = None;
                        self.nparam = self.nparam.next();
                    }
                    self.st = State::DcsParam;
                } else if b == StreamByte::QUESTION {
                    self.private = Marker::Private;
                } else if b.is_csi_intermediate() {
                    self.csi_inter = Some(b);
                    self.st = State::DcsIntermediate;
                } else if let Some(f) = FinalByte::from_stream(b) {
                    // The hook: does baud handle this DCS at all? If not the
                    // payload still has to be consumed, just not kept.
                    self.dcs_final = Some(f);
                    self.st = match Dcs::parse(self.csi_inter, f) {
                        Dcs::Unsupported => State::DcsIgnore,
                        _ => State::DcsPassthrough,
                    };
                } else {
                    self.st = State::DcsIgnore;
                }
            }
            State::DcsPassthrough => {
                if b == StreamByte::ESC {
                    self.st = State::DcsEsc;
                } else if b == StreamByte::BEL {
                    self.dispatch_dcs();
                    self.st = self.ground();
                } else {
                    self.osc_push(b);
                }
            }
            State::DcsIgnore => {
                if b == StreamByte::ESC {
                    // Not `DcsEsc`: there is nothing to dispatch, so ST and a
                    // cancel are the same outcome here.
                    self.st = State::Escape;
                } else if b == StreamByte::BEL {
                    self.st = self.ground();
                }
            }
            State::DcsEsc => {
                if b == StreamByte::ST_FINAL {
                    self.dispatch_dcs();
                    self.st = self.ground();
                } else {
                    // `[[source: williams]]`: a bare ESC cancels the control
                    // string. The payload is dropped, not acted on.
                    self.osc_reset();
                    self.dcs_final = None;
                    self.st = State::Escape;
                    self.control(b);
                }
            }
            // `ESC Y row col` — two bias-0x20 coordinate bytes, collected
            // here because they are parser state, not parameters.
            State::Vt52Row => {
                self.st = State::Vt52Col(Vt52Coord::from_wire(b));
            }
            State::Vt52Col(row) => {
                self.vt52_address(row, b);
                self.st = self.ground();
            }
            State::OscEsc => {
                if b == StreamByte::ST_FINAL {
                    self.osc_term = StringTerm::St;
                    self.dispatch_osc();
                    self.st = self.ground();
                } else {
                    // `[[source: williams]]`: "receiving the ESC character will
                    // cancel the control string". The payload is dropped, not
                    // dispatched, and the byte begins a new escape.
                    self.osc_reset();
                    self.st = State::Escape;
                    self.control(b);
                }
            }
            State::StringIgnore => {
                // Terminated by ST (`ESC \`) or, as xterm accepts, by BEL.
                // Any ESC leaves the string; the Escape state decides whether
                // the next byte is the ST backslash or a fresh sequence, so a
                // truncated string cannot swallow what follows it forever.
                if b == StreamByte::BEL {
                    self.st = self.ground();
                } else if b == StreamByte::ESC {
                    self.st = State::Escape;
                }
            }
            State::CsiIntermediate => {
                if let Some(f) = FinalByte::from_stream(b) {
                    self.dispatch(f);
                    self.st = self.ground();
                } else if !b.is_csi_intermediate() {
                    self.st = State::CsiIgnore;
                }
            }
            State::CsiIgnore => {
                // Consume to the final byte and dispatch nothing.
                if b.is_c0() {
                    self.execute(b);
                } else if FinalByte::from_stream(b).is_some() {
                    self.st = self.ground();
                }
            }
            // `[[source: williams]]`'s `csi_entry`. Its whole reason to exist
            // is the marker rule: `3C-3F` is collected *here* and rejected in
            // `csi_param`. See `State::CsiEntry`.
            State::CsiEntry => {
                if let Some(m) = Marker::from_private(b) {
                    self.private = m;
                    self.st = State::CsiParams;
                } else {
                    // Everything else behaves exactly as it does one byte
                    // later, so it is the same code -- the states differ in
                    // one transition and should differ in one branch.
                    self.st = State::CsiParams;
                    self.csi_param_byte(b);
                }
            }
            State::CsiParams => self.csi_param_byte(b),
            State::OscString => {
                if b == StreamByte::BEL {
                    self.osc_term = StringTerm::Bel;
                    self.dispatch_osc();
                    self.st = self.ground();
                } else if b == StreamByte::ESC {
                    self.st = State::OscEsc;
                } else {
                    self.osc_push(b);
                }
            }
        }
    }

    /// One byte of a CSI parameter string. `[[source: williams]]`'s `csi_param`
    /// transitions, shared with `csi_entry` because they agree on all of them
    /// except the private marker.
    #[inline]
    fn csi_param_byte(&mut self, b: StreamByte) {
        if b.is_c0() {
            // `[[source: williams]]`: C0 inside a control sequence is executed
            // where it appears and the sequence continues. This used to fall
            // through to `csi_ignore`, so `CSI 1 <BEL> m` discarded the SGR --
            // a control character silently cancelling an unrelated operation.
            self.execute(b);
        } else if let Some(d) = b.as_ascii_digit() {
            if self.nparam.is_first() {
                self.nparam = self.nparam.next();
            }
            let cur = self.params[self.nparam.raw_index() - 1].unwrap_or(Param::ZERO);
            self.params[self.nparam.raw_index() - 1] = Some(cur.push(d));
        } else if b == StreamByte::SEMICOLON || b == StreamByte::COLON {
            if self.nparam.is_first() {
                self.nparam = self.nparam.next();
            }
            if self.nparam.raw_index() < 16 {
                // A colon opens a *sub*-parameter of the same attribute.
                // Recorded rather than flattened: `38:2:Pi:r:g:b` has a
                // colour-space identifier that `38;2;r;g;b` does not.
                if b == StreamByte::COLON {
                    self.psub = self.psub.mark(self.nparam);
                }
                self.params[self.nparam.raw_index()] = None;
                self.nparam = self.nparam.next();
            }
        } else if let Some(f) = FinalByte::from_stream(b) {
            self.dispatch(f);
            self.st = self.ground();
        } else if b.is_csi_intermediate() {
            self.csi_inter = Some(b);
            self.st = State::CsiIntermediate;
        } else if b == StreamByte::DEL {
            // `[[source: williams]]` ignores DEL here: not a parameter, not an
            // error. Treating it as malformed would discard a valid sequence.
        } else {
            // A private marker past first position lands here, which is the
            // point -- `[[source: williams]]` sends `3C-3F` to `csi_ignore`
            // from `csi_param`. Consume to the final byte rather than returning
            // to Ground, which printed the remainder of the sequence as text.
            self.st = State::CsiIgnore;
        }
    }

    /// `ESC Fe`: two-byte escapes with no parameters. Shared by both feed paths.
    #[inline(always)]
    pub(crate) fn esc_fe(&mut self, b: StreamByte) {
        match Esc::parse(b) {
            Esc::SaveCursor => {
                self.saved = Some((self.cy, self.cx, self.style, self.style_raw, self.flags))
            }
            // `[[source: vt510-decsc]]` lists the selective erase attribute
            // among what DECSC saves, so it is restored here with the rendition
            // rather than left behind.
            Esc::RestoreCursor => {
                if let Some((r, c, st, sw, fl)) = self.saved {
                    self.cy = r;
                    self.cx = c;
                    self.style = st;
                    self.style_raw = sw;
                    self.flags = fl;
                }
            }
            Esc::ReverseIndex => self.reverse_index(),
            Esc::Index => self.index(),
            Esc::NextLine => self.wrap(),
            Esc::Reset => self.reset(),
            // These were in the fast path's `known` list and then fell through
            // `Esc::parse` to nothing: recognised, consumed, and dropped, which
            // is the `parsed` failure docs/vt-coverage.md is built around.
            Esc::KeypadApplication => self.keypad = KeypadMode::Application,
            Esc::KeypadNumeric => self.keypad = KeypadMode::Numeric,
            Esc::TabSet => {
                let x = self.cx.index();
                self.s.tabs[x] = true;
            }
            Esc::Unsupported => self.unsupported.inc(),
        }
    }
}
