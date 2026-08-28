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
                    // The buffer is about to be cleared, so a rotation left
                    // over from the last time this application was in the
                    // alternate screen has nothing to describe. Blanks rotate
                    // to blanks, so this is tidiness rather than a fix -- but
                    // an invariant that holds only by accident is one the next
                    // change breaks.
                    self.alt_rot = RowRot::NONE;
                    self.clear_screen();
                    self.cx = Col::ZERO;
                    self.cy = Row::ZERO;
                } else if !on && self.screen == Screen::Alternate {
                    self.screen = Screen::Normal;
                    // Through the same clamped path as DECRC, because the
                    // quotation above says this *is* DECRC. The save may
                    // predate a resize, and an unclamped restore puts `cy`
                    // outside the grid -- which writes and erases index with.
                    let saved = self.alt_saved.take();
                    self.place_saved(saved);
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
                // Resetting `?1006` returns to X10, which is the protocol's
                // default rather than "off" -- there is no off.
                self.mouse_encoding = if on {
                    MouseEncoding::Sgr
                } else {
                    MouseEncoding::X10
                }
            }
            // Input-side and rendering modes: recorded, and exposed by the
            // accessors below. The core has no keyboard and no renderer, so
            // exposing them *is* acting on them -- an embedder has no other
            // channel. Same argument as bracketed paste.
            // DECANM. Reset enters VT52; set returns to ANSI. Note the
            // inverted sense -- `on` here means "ANSI mode on".
            PrivateMode::AnsiMode => self.set_terminal_mode(if on {
                TerminalMode::Ansi
            } else {
                TerminalMode::Vt52
            }),
            PrivateMode::CursorKeys => self.modes.apply(ModeBit::CursorKeys, on),
            PrivateMode::SmoothScroll => self.modes.apply(ModeBit::SmoothScroll, on),
            PrivateMode::ReverseVideo => self.modes.apply(ModeBit::ReverseVideo, on),
            PrivateMode::AutoRepeat => self.modes.apply(ModeBit::AutoRepeat, on),
            PrivateMode::MetaSendsEscape => self.modes.apply(ModeBit::MetaSendsEscape, on),
            PrivateMode::AltSendsEscape => self.modes.apply(ModeBit::AltSendsEscape, on),
            PrivateMode::EightBitInput => self.modes.apply(ModeBit::EightBitInput, on),
            // `?45`: backspace at the left margin wraps to the end of the line
            // above instead of stopping. Acted on in `Term::back`.
            PrivateMode::ReverseWrap => self.modes.apply(ModeBit::ReverseWrap, on),
            // `?12` is the blink axis of DECSCUSR, reached by a different
            // route. Setting it must not change the *shape*, so it maps onto
            // the current style rather than replacing it.
            PrivateMode::CursorBlink => self.cursor_style = self.cursor_style.with_blink(on),
            // DECCOLM. `[[source: vt510-decscpp]]` behaviour that is baud's to
            // perform -- clear the screen, reset the margins, home the cursor --
            // happens here. The *resize* does not: geometry is caller-owned
            // through `Term::resize`, so the request is recorded and
            // `Term::column_mode()` reports it. Resizing behind the caller's
            // back would desynchronise it from the application.
            PrivateMode::Column132 => {
                self.modes.apply(ModeBit::Column132, on);
                self.scroll_top = Row::ZERO;
                self.scroll_bot = Row::from_index(self.rows.index() - 1);
                self.reset_side_margins();
                self.cx = Col::ZERO;
                self.cy = Row::ZERO;
                self.clear_screen();
            }
            // `?47` is the pre-1049 alternate screen: it switches buffers and
            // does **not** save or restore the cursor. `?1049` exists because
            // applications kept getting that wrong, so the two are deliberately
            // not aliases.
            PrivateMode::AltScreenLegacy => {
                self.screen = if on {
                    Screen::Alternate
                } else {
                    Screen::Normal
                };
            }
            // `?1001`. Recorded with an accessor, which is what every
            // input-side mode gets: it is the only channel a core with no
            // keyboard and no renderer has. The hang ctlseqs warns of is an
            // event loop ignoring events until a reply arrives, and baud has
            // no event loop -- whether to wait is the embedder's decision.
            PrivateMode::HighlightTracking => {
                self.highlight = if on {
                    HighlightTracking::On
                } else {
                    HighlightTracking::Off
                }
            }
            PrivateMode::BracketedPaste => self.modes.apply(ModeBit::BracketedPaste, on),
            PrivateMode::SynchronizedOutput => self.modes.apply(ModeBit::SynchronizedOutput, on),
            // A mode joins `Modes` only when something can act on it -- but an
            // unknown one is still a sequence accepted and dropped, so it is
            // counted rather than ignored.
            _ => self.unsupported.inc(),
        }
    }

    /// The combining marks attached to a cell, if any.
    ///
    /// A renderer needs these to draw `é` as one glyph rather than as `e`: the
    /// cell holds the base and this holds what combines with it. Empty for the
    /// overwhelming majority of cells, which is why the marks live in a side
    /// table rather than in `PackedCell`'s eight measured bytes.
    pub fn marks(&self, r: Row, c: Col) -> impl Iterator<Item = Codepoint> + '_ {
        let slot = &self.s.marks[MarkSlot::index_of(r, c, NMARK)];
        slot.marks().take(if slot.is_for(r, c) {
            MarkSlot::CAPACITY
        } else {
            0
        })
    }

    /// East Asian Ambiguous width policy.
    ///
    /// Owned by the embedder, not by the application: `[[source: uax11]]` § 5
    /// makes ambiguous width a property of the *reader's* context, and no
    /// escape sequence carries it. xterm exposes the same choice as the
    /// `cjkWidth` resource, defaulting false, which is `Narrow` here.
    ///
    /// Changing it changes how already-written cells would be measured, so set
    /// it before feeding, the way a terminal reads its locale at startup.
    pub fn set_ambiguous_width(&mut self, a: AmbiguousWidth) {
        self.width.ambiguous = a;
    }

    /// What [`Term::set_ambiguous_width`] last selected.
    pub fn ambiguous_width(&self) -> AmbiguousWidth {
        self.width.ambiguous
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

    /// Apply every parameter of a DECSET/DECRST.
    ///
    /// `#[inline(never)]`, for the reason `Term::open_gap` carries: `dispatch`
    /// is inlined into the CSI fast path, and this body copies a 64-byte
    /// parameter array. Inlining it there enlarges the hot dispatch for the
    /// benefit of a branch that SGR-heavy streams never take.
    ///
    /// The copy is needed because `set_private_mode` takes `&mut self` while
    /// the parameters live in `self`.
    #[inline(never)]
    pub(crate) fn apply_private_modes(&mut self, change: ModeChange) {
        let (ps, n) = (self.params, self.nparams());
        for p in ps.iter().take(n) {
            self.set_private_mode(PrivateMode::from_wire(*p), change.is_set());
        }
    }

    /// As [`Term::apply_private_modes`], for the unprefixed namespace.
    #[inline(never)]
    pub(crate) fn apply_ansi_modes(&mut self, change: ModeChange) {
        let (ps, n) = (self.params, self.nparams());
        for p in ps.iter().take(n) {
            self.set_ansi_mode(AnsiMode::from_wire(*p), change.is_set());
        }
    }

    /// DECCKM ?1. When set the arrow keys send `SS3 A` rather than `CSI A`.
    pub fn cursor_keys_application(&self) -> bool {
        self.modes.is_set(ModeBit::CursorKeys)
    }

    /// DECSCNM ?5. When set the whole screen renders with fg and bg swapped.
    pub fn reverse_video(&self) -> bool {
        self.modes.is_set(ModeBit::ReverseVideo)
    }

    /// DECSCLM ?4. A hint that scrolling should be smooth rather than jump.
    pub fn smooth_scroll(&self) -> bool {
        self.modes.is_set(ModeBit::SmoothScroll)
    }

    /// DECARM ?8. Whether the keyboard should auto-repeat.
    pub fn auto_repeat(&self) -> bool {
        self.modes.is_set(ModeBit::AutoRepeat)
    }

    /// `?1036`. Whether the Meta modifier prefixes `ESC`. Read by
    /// [`Term::key`], and set on a fresh terminal.
    pub fn meta_sends_escape(&self) -> bool {
        self.modes.is_set(ModeBit::MetaSendsEscape)
    }

    /// `?1039`. Whether the Alt modifier prefixes `ESC`. Read by
    /// [`Term::key`], and set on a fresh terminal.
    ///
    /// Separate from [`Term::meta_sends_escape`] because `[[source: ctlseqs]]`
    /// line 2337 gives the two resources separate columns.
    pub fn alt_sends_escape(&self) -> bool {
        self.modes.is_set(ModeBit::AltSendsEscape)
    }

    /// `?1034`. Whether Alt/Meta add 128 rather than prefixing `ESC`.
    ///
    /// This is what terminfo's `smm` and `rmm` send, which is why it is
    /// implemented rather than refused: every `xterm*` entry carries them, so
    /// leaving `?1034` unrecognised left two capabilities of the corpus
    /// dropped, and `tests/conformance` counted them.
    ///
    /// **Reading it is not the same as acting on it.** [`Term::key`] applies
    /// the shift only under [`InputEncoding::Latin1`]: a byte above 127 is a
    /// character there and a continuation byte under UTF-8, so applying it in
    /// UTF-8 would send something no application can decode. The mode is
    /// recorded and reported truthfully in both, and the encoding decides.
    pub fn eight_bit_input(&self) -> bool {
        self.modes.is_set(ModeBit::EightBitInput)
    }

    /// DECCOLM ?3. Whether the application asked for 132 columns.
    ///
    /// baud does **not** resize itself: geometry is caller-owned through
    /// [`Term::resize`], and resizing behind the caller would desynchronise it
    /// from the application. The clear, margin reset and cursor home that
    /// DECCOLM mandates *do* happen.
    pub fn column_mode(&self) -> bool {
        self.modes.is_set(ModeBit::Column132)
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
            // `?1020` mirrors the **live** encoding, which is what xterm does:
            // `do_dec_rqm` answers `MdBool` from `screen->utf8_mode`. It
            // answered `PermanentlySet` while `ESC % @` was refused, and that
            // was true then; implementing the switch made it false, so it moved
            // in the same commit. A mode report that outlives the fact it
            // reported is the failure this file exists to prevent.
            //
            // **A named divergence from xterm, and deliberate.** xterm answers
            // this with 1/2 (set/reset) even though its own documentation marks
            // the resource read-only. baud answers 3 (permanently set), because
            // 1 would invite an application to send `CSI ? 1020 l`, get no
            // error, and believe UTF-8 decoding had been turned off -- the
            // exact failure `docs/vt-coverage.md` refuses for DECRQM. DECRPM
            // has value 3 for precisely this case.
            PrivateMode::HighlightTracking => match self.highlight {
                HighlightTracking::On => ModeState::Set,
                HighlightTracking::Off => ModeState::Reset,
            },
            PrivateMode::Utf8Resource => match self.encoding {
                InputEncoding::Utf8 => ModeState::Set,
                InputEncoding::Latin1 => ModeState::Reset,
            },
            // `?2027` answers 3 for the same reason `?1020` does: baud
            // clusters per `[[source: uax29]]` and has no mode that stops it,
            // so `1` would invite an application to try turning it off.
            // It answered `0` while clustering was unimplemented, which was the
            // honest answer *then* -- saying `2` would have claimed the mode
            // existed and was merely off.
            PrivateMode::GraphemeClustering => ModeState::PermanentlySet,
            PrivateMode::Origin => bit_state(self.modes, ModeBit::Origin),
            PrivateMode::AutoWrap => bit_state(self.modes, ModeBit::AutoWrap),
            PrivateMode::LeftRightMargin => bit_state(self.modes, ModeBit::SideMargins),
            PrivateMode::AnsiMode => match self.terminal_mode {
                TerminalMode::Ansi => ModeState::Set,
                TerminalMode::Vt52 => ModeState::Reset,
            },
            PrivateMode::CursorKeys => bit_state(self.modes, ModeBit::CursorKeys),
            PrivateMode::SmoothScroll => bit_state(self.modes, ModeBit::SmoothScroll),
            PrivateMode::ReverseVideo => bit_state(self.modes, ModeBit::ReverseVideo),
            PrivateMode::AutoRepeat => bit_state(self.modes, ModeBit::AutoRepeat),
            PrivateMode::MetaSendsEscape => bit_state(self.modes, ModeBit::MetaSendsEscape),
            PrivateMode::AltSendsEscape => bit_state(self.modes, ModeBit::AltSendsEscape),
            PrivateMode::EightBitInput => bit_state(self.modes, ModeBit::EightBitInput),
            PrivateMode::ReverseWrap => bit_state(self.modes, ModeBit::ReverseWrap),
            PrivateMode::Column132 => bit_state(self.modes, ModeBit::Column132),
            PrivateMode::CursorBlink => {
                if self.cursor_style.blinks() {
                    ModeState::Set
                } else {
                    ModeState::Reset
                }
            }
            PrivateMode::AltScreenLegacy => match self.screen {
                Screen::Alternate => ModeState::Set,
                Screen::Normal => ModeState::Reset,
            },
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
                MouseEncoding::X10 => ModeState::Reset,
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
            // A mode joins `Modes` only when something can act on it -- but an
            // unknown one is still a sequence accepted and dropped, so it is
            // counted rather than ignored. The private namespace already did
            // this; the ANSI one did not.
            AnsiMode::Other(_) => self.unsupported_param.inc(),
        }
    }

    /// DECKPAM/DECKPNM: what the numeric keypad should send.
    pub fn keypad(&self) -> KeypadMode {
        self.keypad
    }
}
