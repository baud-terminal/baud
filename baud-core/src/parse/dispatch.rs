//! Walking the graph: one lookup, then one flat `match` over the action.
//!
//! `machine.rs` answers "what does this byte mean" by walking a `match` over
//! twenty-six nodes with a chain of byte tests inside each arm. Every one of
//! those tests is a pure function of `(encoding, node, byte)`, so
//! `parse::graph` answers it at build time and this file only performs what
//! the answer names.
//!
//! **The node is set before the action runs, and some actions overwrite it.**
//! That order is not incidental. `CSI ? 2 l` enters VT52 from an ANSI
//! sequence, so `set_terminal_mode` has to be able to move the parser to the
//! other sub-graph *after* the edge has named this one; `vt52_dispatch`
//! decides for itself whether `ESC Y` takes two more bytes; `dcs_hook` chooses
//! between passthrough and ignore on the collected intermediate. Those three
//! own their transitions because where they go depends on something that is
//! not the indexed byte, and `the_table_sends_every_byte_where_the_machine_does`
//! exempts exactly those cells for the same reason.
//!
//! **`anywhere` is gone from the byte path.** `[[source: williams]]`'s
//! anywhere block -- `CAN`, `SUB` and `ESC` meaning the same thing from almost
//! every state -- was a test before the match, with a `b.is_c0()` gate in
//! front of it because the naive three comparisons cost 6% of the streaming
//! cells path. In a graph it is not a test at all: those bytes have edges like
//! any other, written once when the graph is built.

use crate::*;

impl Term<'_> {
    /// One byte of control input.
    #[inline(always)]
    pub(crate) fn control(&mut self, b: StreamByte) {
        let e = Edge::of(self.encoding, self.st, b);
        self.st = e.next;
        self.act(e.act, b);
    }

    /// Perform what the edge named.
    ///
    /// Out of line and not inlined into [`Term::control`]: the arms between
    /// them are most of the protocol, and `control` is reached from the
    /// `#[inline(always)]` ingest loop, where this repo has measured that
    /// growing the inlined body costs more than the work in it does -- 528
    /// bytes of `feed_with` against ~5%, and `anywhere_c0` carries the same
    /// annotation for the same reason.
    #[inline(never)]
    fn act(&mut self, a: Act, b: StreamByte) {
        match a {
            Act::Nothing => {}
            Act::Execute => self.execute(b),

            // ── ground ────────────────────────────────────────────────────
            Act::PrintByte => {
                // ISO 8859-1: one byte, one character, no decoder state -- and
                // through the designated set, because G0-G3 designation
                // applies to the byte and not to the route it arrived on.
                if self.encoding.single_byte(b).is_some() {
                    let c = PackedCell::translate(b, self.charset());
                    let w = Width::of(c, self.width.ambiguous);
                    self.put_cp(c, w);
                }
            }
            Act::C1Escape => {
                // **8-bit C1, which only exists outside UTF-8.** In UTF-8
                // `9B` is a continuation byte and cannot be a control; in ISO
                // 8859-1 it is unambiguously CSI. Dispatched through the
                // escape node rather than duplicating its edges:
                // `[[source: ctlseqs]]` calls `ESC [` "a 7-bit equivalent to
                // 8-bit" `9B`, so the two forms are the same operation and
                // must not be able to drift apart.
                if let Some(fe) = self.encoding.c1_as_escape(b) {
                    self.control(fe);
                }
            }
            Act::GroundUncommon => self.ground_uncommon(b),

            // ── UTF-8 ─────────────────────────────────────────────────────
            Act::Utf8Start => {
                #[cfg(feature = "instrument")]
                self.bytes_utf8.inc();
                if let Some((_, acc, min)) = b.utf8_lead() {
                    self.utf8_acc = Utf8Acc::lead(acc);
                    self.utf8_min = min;
                }
            }
            Act::Utf8Cont => {
                #[cfg(feature = "instrument")]
                self.bytes_utf8.inc();
                if let Some(bits) = b.utf8_continuation() {
                    let acc = self.utf8_acc.push(bits);
                    self.utf8_acc = acc;
                    // The edge has already left the UTF-8 family when this was
                    // the last continuation, so "am I finished" is "where did
                    // I land" -- the count is the node.
                    if !self.st.is_utf8() {
                        // `finish` rejects a non-shortest form. Without that
                        // check every overlong three- and four-byte sequence
                        // decoded to a real character -- `E0 80 AF` rendered
                        // `/`, the path-traversal overlong.
                        let c = acc.finish(self.utf8_min);
                        if c == Codepoint::REPLACEMENT {
                            self.replacements.inc();
                        }
                        let w = Width::of(c, self.width.ambiguous);
                        self.put_cp(c, w);
                    }
                }
            }
            Act::Utf8Bad => {
                self.replacements.inc();
                self.put_cp(Codepoint::REPLACEMENT, Width::NARROW);
                // Re-dispatched from ground, which the edge has already named.
                self.control(b);
            }

            // ── escape ────────────────────────────────────────────────────
            Act::EscFe => self.esc_fe(b),
            Act::EscInterFirst => {
                self.esc_inter = Some(b);
                self.esc_inter2 = None;
            }
            Act::EscCollect => self.esc_collect(b),
            Act::EscInterFinal => self.esc_inter_final(b),

            // ── control sequences ─────────────────────────────────────────
            Act::CsiClear => self.csi_clear(),
            Act::CsiPrivate => {
                if let Some(m) = Marker::from_private(b) {
                    self.private = m;
                }
            }
            Act::CsiParamByte => self.csi_collect_param(b),
            Act::CsiCollect => self.csi_inter = Some(b),
            Act::CsiFinal => {
                if let Some(f) = FinalByte::from_stream(b) {
                    self.dispatch(f);
                }
            }

            // ── device control strings ────────────────────────────────────
            Act::DcsClear => {
                self.csi_clear();
                self.osc_reset();
            }
            Act::DcsPrivate => {
                if let Some(m) = Marker::from_private(b) {
                    self.private = m;
                }
            }
            Act::DcsParamByte => self.dcs_collect_param(b),
            Act::DcsCollect => self.csi_inter = Some(b),
            Act::DcsHook => {
                if let Some(f) = FinalByte::from_stream(b) {
                    self.dcs_hook(f);
                }
            }
            Act::DcsDispatch => self.dispatch_dcs(),
            Act::DcsCancel => {
                // `[[source: williams]]`: a bare ESC cancels the control
                // string. The payload is dropped, not acted on.
                self.osc_reset();
                self.dcs_final = None;
                self.control(b);
            }

            // ── strings ───────────────────────────────────────────────────
            Act::StringPut => self.osc_push(b),
            Act::OscDispatchSt => {
                self.osc_term = StringTerm::St;
                self.dispatch_osc();
            }
            Act::OscDispatchBel => {
                self.osc_term = StringTerm::Bel;
                self.dispatch_osc();
            }
            Act::OscCancel => {
                self.osc_reset();
                self.control(b);
            }

            // ── anywhere ──────────────────────────────────────────────────
            Act::Cancel => {
                // Abandon whatever is in progress, control strings included. A
                // payload that cannot be cancelled is the same hazard as one
                // that is executed.
                self.execute(b);
                self.osc_reset();
            }

            // ── VT52 ──────────────────────────────────────────────────────
            Act::Vt52Dispatch => self.vt52_dispatch(b),
            Act::Vt52SetRow => self.vt52_row = Vt52Coord::from_wire(b),
            Act::Vt52Address => self.vt52_address(self.vt52_row, b),
        }
    }

    /// Clear the parameter block. `CSI` and `DCS` both start with it.
    #[inline(always)]
    fn csi_clear(&mut self) {
        self.params = [None; ParamIdx::CAPACITY];
        self.nparam = ParamIdx::FIRST;
        self.psub = SubParams::NONE;
        self.private = Marker::Standard;
        self.csi_inter = None;
    }
}
