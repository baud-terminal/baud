use crate::*;

/// Whether `level` is the tracking currently selected.
///
/// The three modes are exclusive on the wire, so DECRQM answers "set" only for
/// the one actually in force -- reporting all three as set would tell an
/// application it is getting motion events it is not.
pub(crate) fn tracking_state(cur: MouseTracking, level: MouseTracking) -> ModeState {
    if cur == level {
        ModeState::Set
    } else {
        ModeState::Reset
    }
}

/// [`ModeState`] for one bit, so `mode_state` reads as a table rather than as
/// eight copies of the same `if`.
pub(crate) fn bit_state(m: Modes, b: ModeBit) -> ModeState {
    if m.is_set(b) {
        ModeState::Set
    } else {
        ModeState::Reset
    }
}

impl<'a> Term<'a> {
    /// DECSET / DECRST.
    ///
    /// Only modes baud acts on are stored, for the same reason as the ANSI
    /// ones: a bit that is set and never read is the "accepted, did nothing"
    /// failure `docs/vt-coverage.md` calls `parsed`. Mouse reporting, bracketed
    /// paste and the alternate screen are recognised by the parser and
    /// deliberately not recorded here -- they are input-side or need a second
    /// screen buffer, and pretending otherwise would be worse than ignoring.
    pub(crate) fn set_private_mode(&mut self, m: PrivateMode, on: bool) {
        match m {
            PrivateMode::CursorVisible => self.modes.apply(ModeBit::CursorVisible, on),
            PrivateMode::Origin => {
                self.modes.apply(ModeBit::Origin, on);
                // Both DECSET and DECRST home the cursor, and where home *is*
                // depends on the mode just applied.
                self.cx = Col::ZERO;
                self.cy = if on { self.scroll_top } else { Row::ZERO };
            }
            PrivateMode::AutoWrap => self.modes.apply(ModeBit::AutoWrap, on),
            // `[[source: vt510-decslrm]]`: DECSLRM "only works when vertical
            // split screen mode (DECLRMM) is set". Turning DECLRMM off puts the
            // margins back to the page borders -- leaving them set while the
            // mode that gives them meaning is off would confine writes with
            // nothing to explain why.
            PrivateMode::LeftRightMargin => {
                self.modes.apply(ModeBit::SideMargins, on);
                if !on {
                    self.reset_side_margins();
                }
            }
            // `[[source: ctlseqs]]` for `?1049 h`: "Save cursor as in DECSC ...
            // After saving the cursor, switch to the Alternate Screen Buffer,
            // clearing it first." And for `?1049 l`: "Use Normal Screen Buffer
            // and restore cursor as in DECRC."
            //
            // Idempotent in both directions. A second `?1049 h` must not
            // re-save the cursor over the one it saved on the way in, or an
            // application that sets the mode twice loses the position it will
            // be restored to.
            PrivateMode::AltScreen => {
                if on && self.screen == Screen::Normal {
                    self.alt_saved =
                        Some((self.cy, self.cx, self.style, self.style_raw, self.flags));
                    self.screen = Screen::Alternate;
                    self.clear_screen();
                    self.cx = Col::ZERO;
                    self.cy = Row::ZERO;
                } else if !on && self.screen == Screen::Alternate {
                    self.screen = Screen::Normal;
                    if let Some((r, c, st, sw, fl)) = self.alt_saved.take() {
                        self.cy = r;
                        self.cx = c;
                        self.style = st;
                        self.style_raw = sw;
                        self.flags = fl;
                    }
                }
            }
            // The three tracking modes are a *level*, not three flags: xterm
            // treats each DECSET as selecting one, and 1003 is a superset of
            // 1002 which is a superset of 1000. Resetting any of them turns
            // tracking off, which is what applications expect from `?1000 l`.
            PrivateMode::MouseNormal => self.set_tracking(MouseTracking::Normal, on),
            PrivateMode::MouseButtonEvent => self.set_tracking(MouseTracking::ButtonEvent, on),
            PrivateMode::MouseAnyEvent => self.set_tracking(MouseTracking::AnyEvent, on),
            PrivateMode::MouseSgr => {
                self.mouse_encoding = if on {
                    MouseEncoding::Sgr
                } else {
                    MouseEncoding::None
                }
            }
            PrivateMode::BracketedPaste => self.modes.apply(ModeBit::BracketedPaste, on),
            PrivateMode::SynchronizedOutput => self.modes.apply(ModeBit::SynchronizedOutput, on),
            // Still deliberately unrecorded. A bit that is set and never read
            // is the same failure as a sequence parsed and dropped, so a mode
            // joins `Modes` only when something can act on it.
            _ => {}
        }
    }

    /// Whether the cursor should be drawn. DECTCEM; a renderer needs it.
    pub fn cursor_visible(&self) -> bool {
        self.modes.is_set(ModeBit::CursorVisible)
    }

    /// The cursor's shape and blink. DECSCUSR.
    pub fn cursor_style(&self) -> CursorStyle {
        self.cursor_style
    }

    /// DECSET ?2004. When set, the embedder must bracket pasted text with
    /// `ESC [ 200 ~` and `ESC [ 201 ~` so the application can tell a paste from
    /// typing -- without it, a pasted newline runs as a command.
    pub fn bracketed_paste(&self) -> bool {
        self.modes.is_set(ModeBit::BracketedPaste)
    }

    /// DECSET ?2026. While set, a renderer should keep showing the last
    /// complete frame instead of drawing a half-applied update.
    pub fn synchronized_output(&self) -> bool {
        self.modes.is_set(ModeBit::SynchronizedOutput)
    }

    /// What DECRQM should answer about `mode` in `marker`'s namespace.
    ///
    /// Every mode baud does not implement answers `NotRecognized`, which is the
    /// honest answer and the useful one: an application asks DECRQM precisely
    /// to find out whether a mode exists. Saying "reset" about a mode that does
    /// not exist would tell it the mode is available, and it would then set the
    /// mode and believe it took effect.
    pub(crate) fn mode_state(&self, marker: Marker, mode: Param) -> ModeState {
        if marker != Marker::Private {
            return match AnsiMode::from_wire(Some(mode)) {
                AnsiMode::LineFeedNewLine => bit_state(self.modes, ModeBit::LineFeedNewLine),
                AnsiMode::InsertReplace => bit_state(self.modes, ModeBit::InsertReplace),
                _ => ModeState::NotRecognized,
            };
        }
        match PrivateMode::from_wire(Some(mode)) {
            PrivateMode::Origin => bit_state(self.modes, ModeBit::Origin),
            PrivateMode::AutoWrap => bit_state(self.modes, ModeBit::AutoWrap),
            PrivateMode::LeftRightMargin => bit_state(self.modes, ModeBit::SideMargins),
            PrivateMode::CursorVisible => bit_state(self.modes, ModeBit::CursorVisible),
            PrivateMode::BracketedPaste => bit_state(self.modes, ModeBit::BracketedPaste),
            PrivateMode::SynchronizedOutput => bit_state(self.modes, ModeBit::SynchronizedOutput),
            // Not a `Modes` bit -- the alt screen is a whole buffer, not a
            // flag -- but it is implemented, so DECRQM must not deny it.
            PrivateMode::MouseNormal => tracking_state(self.mouse, MouseTracking::Normal),
            PrivateMode::MouseButtonEvent => tracking_state(self.mouse, MouseTracking::ButtonEvent),
            PrivateMode::MouseAnyEvent => tracking_state(self.mouse, MouseTracking::AnyEvent),
            PrivateMode::MouseSgr => match self.mouse_encoding {
                MouseEncoding::Sgr => ModeState::Set,
                MouseEncoding::None => ModeState::Reset,
            },
            PrivateMode::AltScreen => {
                if self.screen == Screen::Alternate {
                    ModeState::Set
                } else {
                    ModeState::Reset
                }
            }
            _ => ModeState::NotRecognized,
        }
    }

    /// SM / RM for the unprefixed namespace.
    ///
    /// Only modes baud acts on are stored. An unrecognised mode is ignored
    /// rather than recorded, so a bit can never be set and never read.
    pub(crate) fn set_ansi_mode(&mut self, m: AnsiMode, on: bool) {
        match m {
            AnsiMode::LineFeedNewLine => self.modes.apply(ModeBit::LineFeedNewLine, on),
            AnsiMode::InsertReplace => self.modes.apply(ModeBit::InsertReplace, on),
            AnsiMode::Other(_) => {}
        }
    }

    /// DECKPAM/DECKPNM: what the numeric keypad should send.
    pub fn keypad(&self) -> KeypadMode {
        self.keypad
    }
}
