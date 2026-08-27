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
        if self.anywhere(b) {
            return;
        }
        match self.st {
            State::Ground | State::Vt52Ground => {
                if b == StreamByte::ESC {
                    self.st = State::Escape;
                } else if b.is_c0() || b == StreamByte::BEL {
                    self.execute(b);
                } else if let Some((rem, acc, min)) = b.utf8_lead() {
                    self.st = State::Utf8 {
                        remaining: Pending::new(rem),
                        acc: Utf8Acc::lead(acc),
                        min,
                    };
                } else if b.is_printable_ascii() {
                    // Reached only when a byte is *re-fed* here after a failed
                    // decode: the run scanner takes printable ASCII before the
                    // state machine ever sees it, so this arm is cold.
                    //
                    // Without it, `[[source: unicode-ch3]]` § 3.9.5 was
                    // violated. A converter "must not consume the successor
                    // bytes as part of the ill-formed subsequence whenever
                    // those successor bytes themselves constitute part of a
                    // well-formed UTF-8 code unit subsequence", and the
                    // standard names this exact input: `<C2 41 42>` "must not
                    // return `<U+FFFD>` or `<U+FFFD, U+0042>`". baud returned
                    // `<U+FFFD, U+FFFD, U+0042>` -- the `A` fell through to the
                    // not-text arm below and was replaced.
                    self.put_cp(Codepoint::from_ascii(b), Width::NARROW);
                } else {
                    // stray continuation byte or 0x7f: not text
                    self.replacements.inc();
                    self.put_cp(Codepoint::REPLACEMENT, Width::NARROW);
                }
            }
            State::Utf8 {
                remaining,
                acc,
                min,
            } => match b.utf8_continuation() {
                Some(bits) => {
                    let acc = acc.push(bits);
                    if remaining.is_last() {
                        self.st = self.ground();
                        // `finish` rejects a non-shortest form. Without that
                        // check every overlong three- and four-byte sequence
                        // decoded to a real character -- `E0 80 AF` rendered
                        // `/`, the path-traversal overlong.
                        let c = acc.finish(min);
                        if c == Codepoint::REPLACEMENT {
                            self.replacements.inc();
                        }
                        self.put_cp(c, Width::of(c));
                    } else {
                        self.st = State::Utf8 {
                            remaining: remaining.dec(),
                            acc,
                            min,
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
                        // `ESC SP F` is S7C1T: send C1 controls as 7-bit
                        // sequences. baud always does, so this is *satisfied*,
                        // not unsupported -- there is nothing to change and
                        // nothing withheld. dtterm's `is2` opens with it, and
                        // the conformance fixture is what drew the line:
                        // counting it as unsupported claimed baud had refused
                        // something it in fact already does.
                        //
                        // `ESC SP G` (S8C1T) is the opposite and falls through
                        // to the count below: it asks for 8-bit replies, which
                        // would make baud's own output invalid UTF-8.
                        // `ESC SP F` / `ESC SP G` -- S7C1T / S8C1T. Both are
                        // implemented: they select the form baud *sends*, which
                        // is a different axis from the UTF-8 decoding that makes
                        // an 8-bit C1 unparseable on the way in.
                        Some(i) if i == StreamByte::SP && b.raw_byte() == b'F' => {
                            self.c1 = C1Transmit::SevenBit
                        }
                        Some(i) if i == StreamByte::SP && b.raw_byte() == b'G' => {
                            self.c1 = C1Transmit::EightBit
                        }
                        // `ESC % G` selects UTF-8 (`[[source: ctlseqs]]`:
                        // "Select UTF-8 character set, ISO 2022"). baud always
                        // decodes UTF-8, so the request is *satisfied* -- the
                        // same distinction S7C1T draws, and for the same
                        // reason: counting it as unsupported would claim baud
                        // refused something it already does.
                        //
                        // `ESC % @` -- "Select default character set. That is
                        // ISO 8859-1" -- asks to leave UTF-8, which baud cannot
                        // do. It falls through to the count below, so an
                        // application can discover the refusal.
                        Some(i) if i == StreamByte::PERCENT && b.raw_byte() == b'G' => {}
                        // `ESC ( ) * + <C>` designate G0-G3.
                        Some(i) => {
                            if let Some(slot) = GSlot::from_intermediate(i) {
                                self.designate(slot, b);
                            } else {
                                // Every other intermediate form -- `ESC SP F`
                                // and `ESC SP G` (S7C1T/S8C1T) most notably --
                                // was consumed and dropped in silence. Counting
                                // it is the difference between "not
                                // implemented" and "accepted, did nothing, and
                                // gave the application no way to find out",
                                // which is the failure docs/vt-coverage.md is
                                // built around.
                                self.unsupported.inc();
                            }
                        }
                        None => self.unsupported.inc(),
                    }
                    self.st = self.ground();
                }
            }
            // `[[source: williams]]`'s dcs_entry / dcs_param / dcs_intermediate.
            // Collecting the sequence's identity; the final byte decides
            // whether the data that follows is collected or discarded.
            // `[[source: williams]]`'s `dcs_entry`. Split from `dcs_param` for
            // the same reason `csi_entry` is: `3C-3F` is a private marker here
            // and an error one byte later. These three were one merged arm and
            // carried all three of the bugs that merge causes.
            State::DcsEntry => {
                if b.is_c0() || b == StreamByte::DEL {
                    // "C0 controls other than CAN, SUB and ESC are **not
                    // executed** while recognising the first part of a device
                    // control string." Note the contrast with a control
                    // sequence, where the same bytes *are* executed -- same
                    // byte, different state, different rule, which is why the
                    // states are separate. They are ignored, and they do not
                    // consume the marker position.
                } else if let Some(m) = Marker::from_private(b) {
                    self.private = m;
                    self.st = State::DcsParam;
                } else {
                    self.st = State::DcsParam;
                    self.dcs_param_byte(b);
                }
            }
            State::DcsParam => self.dcs_param_byte(b),
            State::DcsIntermediate => {
                if b.is_c0() || b == StreamByte::DEL {
                    // ignored, as above
                } else if b.is_csi_intermediate() {
                    self.csi_inter = Some(b);
                } else if let Some(f) = FinalByte::from_stream(b) {
                    self.dcs_hook(f);
                } else {
                    // `30-3F` after an intermediate: the parameter string is
                    // over, so a digit here is malformed. The merged arm walked
                    // back into `dcs_param` and accepted it.
                    self.st = State::DcsIgnore;
                }
            }
            State::DcsPassthrough => {
                if b == StreamByte::ESC {
                    self.st = State::DcsEsc;
                } else if b == StreamByte::BEL {
                    self.dispatch_dcs();
                    self.st = self.ground();
                } else if b.is_c0() {
                    // "C0 controls other than CAN, SUB and ESC are ignored
                    // during reception of the control string."
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
                if b.is_c0() {
                    // williams gives `csi_intermediate` the same execute action
                    // as `csi_param`. This arm had none, so a C0 fell through
                    // to `csi_ignore` and discarded the sequence: `CSI ! <NUL>
                    // p` lost its DECSTR.
                    self.execute(b);
                } else if let Some(f) = FinalByte::from_stream(b) {
                    self.dispatch(f);
                    self.st = self.ground();
                } else if b == StreamByte::DEL {
                    // ignored, per williams
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
                if b.is_c0() {
                    // `[[source: williams]]`: execute and **stay in
                    // `csi_entry`**. A C0 does not consume the marker
                    // position, so `CSI <BS> ? J` is still DECSED. Advancing
                    // to `CsiParams` first would make the marker rule reject
                    // a marker that is legal.
                    self.execute(b);
                } else if b == StreamByte::DEL {
                    // Ignored, and likewise does not consume the position.
                } else if let Some(m) = Marker::from_private(b) {
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
                } else if b.is_c0() {
                    // `[[source: williams]]`'s `osc_string` takes the *ignore*
                    // action on `01-17,19,1C-1F`; only `20-7F` is `osc_put`.
                    // Appending them put raw control bytes into an
                    // attacker-supplied title -- bytes the terminal had already
                    // been told to drop.
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

    /// One byte of a DCS parameter string. `[[source: williams]]`'s `dcs_param`,
    /// shared with `dcs_entry` because they agree on everything but the marker.
    #[inline]
    fn dcs_param_byte(&mut self, b: StreamByte) {
        if b.is_c0() || b == StreamByte::DEL {
            // Ignored, not executed and not an error. See `State::DcsEntry`.
        } else if let Some(d) = b.as_ascii_digit() {
            if self.nparam.is_first() {
                self.nparam = self.nparam.next();
            }
            let cur = self.params[self.nparam.raw_index() - 1].unwrap_or(Param::ZERO);
            self.params[self.nparam.raw_index() - 1] = Some(cur.push(d));
        } else if b == StreamByte::SEMICOLON {
            if self.nparam.is_first() {
                self.nparam = self.nparam.next();
            }
            if self.nparam.raw_index() < 16 {
                self.params[self.nparam.raw_index()] = None;
                self.nparam = self.nparam.next();
            }
        } else if b.is_csi_intermediate() {
            self.csi_inter = Some(b);
            self.st = State::DcsIntermediate;
        } else if let Some(f) = FinalByte::from_stream(b) {
            self.dcs_hook(f);
        } else {
            // `3A` colon and `3C-3F` past first position. williams sends both
            // to `dcs_ignore`; unlike CSI there is no ctlseqs sub-parameter
            // form to accommodate here, so the spec rule stands unmodified.
            self.st = State::DcsIgnore;
        }
    }

    /// williams' `hook`: the final byte selects a handler, and an unhandled DCS
    /// still has to consume its payload rather than fall back to Ground — or
    /// the payload runs as commands.
    #[inline]
    fn dcs_hook(&mut self, f: FinalByte) {
        self.dcs_final = Some(f);
        self.st = match Dcs::parse(self.csi_inter, f) {
            Dcs::Unsupported => State::DcsIgnore,
            _ => State::DcsPassthrough,
        };
    }

    /// `[[source: williams]]`'s **anywhere** block: the bytes whose meaning does
    /// not depend on which state they arrive in.
    ///
    /// Before the match rather than inside each arm, because four arms had
    /// already forgotten them. `is_c0()` is `b < 0x20`, which *includes* `1B`
    /// ESC, `18` CAN and `1A` SUB — so every arm that wrote
    /// `if b.is_c0() { self.execute(b) }` was executing an ESC and staying
    /// where it was. `CSI 1 ESC [ 2 m` lost its escape and printed `2m`.
    ///
    /// The states that decline are the ones that must: inside a control string
    /// ESC is the first half of ST, so the payload has to survive until the
    /// next byte decides between *terminator* and *cancel*. Those arms spell
    /// the rule out themselves and are already right.
    #[inline(always)]
    fn anywhere(&mut self, b: StreamByte) -> bool {
        // Every byte in the block is below 0x20, so one comparison retires the
        // whole check for all text. This is not premature: `control` is the
        // byte-at-a-time path, and multibyte text runs through it a byte at a
        // time, so the naive three comparisons cost **6%** of the streaming
        // cells path -- 1063.2 [1036.0-1070.5] down to 995.6 [973.3-1020.3],
        // ranges not overlapping. This file already records a per-byte mode
        // check costing 4% and a runtime flag costing 5%; the same rule applies
        // to a per-byte protocol check.
        if !b.is_c0() {
            return false;
        }
        self.anywhere_c0(b)
    }

    /// The rare half of [`Term::anywhere`], deliberately out of line.
    ///
    /// `#[inline(never)]` for the same measured reason `open_gap` is: `control`
    /// is reached from the `#[inline(always)]` ingest loop, and growing its
    /// inlined body costs more than the work in it does. Keeping the gate
    /// inline and the body out of line is what made this free.
    #[inline(never)]
    fn anywhere_c0(&mut self, b: StreamByte) -> bool {
        if b == StreamByte::CAN || b == StreamByte::SUB {
            // Abandon whatever is in progress, control strings included. A
            // payload that cannot be cancelled is the same hazard as one that
            // is executed, and williams gives both bytes the same action from
            // every state.
            if matches!(
                self.st,
                State::Ground | State::Vt52Ground | State::Utf8 { .. }
            ) {
                return false; // nothing in progress; Ground executes it itself
            }
            self.execute(b);
            self.osc_reset();
            self.st = self.ground();
            return true;
        }
        if b != StreamByte::ESC {
            return false;
        }
        match self.st {
            State::Ground
            | State::Vt52Ground
            | State::Utf8 { .. }
            // VT52 coordinates are raw bytes, not a parameter string:
            // `ESC Y` takes the next two whatever they are.
            | State::Vt52Row
            | State::Vt52Col(_)
            // Control strings: ESC is half of ST.
            | State::OscString
            | State::OscEsc
            | State::DcsPassthrough
            | State::DcsEsc
            | State::DcsIgnore
            | State::StringIgnore => false,
            _ => {
                self.st = State::Escape;
                true
            }
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
