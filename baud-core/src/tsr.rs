//! DECRQTSR's two reports and DECRSTS, which restores either: the terminal
//! state report DECTSR and the colour table report DECCTR.
//!
//! `[[source: dec-std-070]]` pp. 5-109 and 5-206 to 5-208; `[[source:
//! vt3xx-tp-002]]` pp. 205-209; `[[source: vt520-rm]]` pp. 5-113, 5-114,
//! 5-117, 5-118 and 5-161. The layout of DECTSR's data is baud's
//! (`baud_domain::TerminalState`): "The format of the data is firmware
//! revision dependent". Answered whatever level DA claims, as DECRQPSR is.
//!
//! **A restore goes through each control's own handler**, a field at a time
//! and only where the report differs from the state now, XTRESTORE's rule:
//! a mode through DECSET or SM, a setting through the control DECRQSS would
//! answer with, the cursor information through DECRSPS's restore. So a
//! restore has every effect the controls have -- `?3` clears, the alternate
//! screen is switched to or from through `?47`, the first of its three
//! modes in slot order, so `?1049`'s cursor save is not replayed, DECSTBM
//! homes the cursor -- and the cursor, restored last, ends where the report
//! put it. The conformance level goes first, since changing it soft-resets,
//! and the input encoding after it, as `ESC % @` and `ESC % G` set it. The
//! tab stops and the shown screen's `DECSC` save, which no control
//! restores, are written directly, the save into the slot of the screen the
//! modes left shown.

use crate::osc::StrKind;
use crate::paint::Paint;
use crate::write::Csi;
use baud_domain::*;

/// One control's answer: the longest of either report.
const _: () = assert!(TSR_REPLY_MAX <= crate::storage::REPLY_MOST);
const _: () = assert!(CTR_REPLY_MAX <= crate::storage::REPLY_MOST);
const _: () = assert!(TSR_DATA_MAX + 2 <= crate::storage::OSC_MAX);

/// What a mode's DECRQM answer is in the report.
fn tsr_mode(s: ModeState) -> TsrMode {
    match s {
        ModeState::Set => TsrMode::Set,
        ModeState::Reset => TsrMode::Reset,
        _ => TsrMode::Absent,
    }
}

/// A small control's bytes, built where it is replayed.
struct Ctl {
    buf: [u8; 64],
    n: ByteLen,
}

impl Ctl {
    fn new() -> Ctl {
        Ctl {
            buf: [0; 64],
            n: ByteLen::ZERO,
        }
    }
    fn push(&mut self, b: &[u8]) -> &mut Ctl {
        let at = self.n.index();
        self.buf[at..at + b.len()].copy_from_slice(b);
        self.n = ByteLen::from_index(at + b.len());
        self
    }
    fn num(&mut self, v: impl Into<i32>) -> &mut Ctl {
        let mut d = [0u8; 6];
        let mut k = 0;
        let mut v = v.into().unsigned_abs();
        loop {
            d[k] = b'0' + u8::try_from(v % 10).unwrap_or(0);
            k += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            self.push(&[d[k]]);
        }
        self
    }
    fn bytes(&self) -> &[u8] {
        &self.buf[..self.n.index()]
    }
}

impl Paint<'_, '_> {
    /// DECRQTSR, `CSI Ps $ u`: 1 the terminal state report, 2 the colour
    /// table report in the system `Pu` names, `CSI 2 ; Pu $ u`; 0, omitted,
    /// and any other `Ps` or `Pu` are ignored, "no report sent" and "the
    /// entire command is ignored" (p. 5-206).
    #[cold]
    #[inline(never)]
    pub(crate) fn request_terminal_state(&mut self, csi: &Csi) {
        let params = csi.params();
        match params.raw(ParamIdx::FIRST, 0) {
            1 => self.report_terminal_state(),
            2 => {
                if let Some(space) = ColorSpace::of_request(params.raw(ParamIdx::SECOND, 0)) {
                    self.report_color_table(space);
                }
            }
            _ => {}
        }
    }

    /// The state DECTSR reports: what DECRQM, DECRQSS and DECRQPSR answer
    /// now.
    pub(crate) fn terminal_state(&self) -> TerminalState {
        let mut ansi = [TsrMode::Absent; TSR_ANSI_MODES.len()];
        for (m, w) in ansi.iter_mut().zip(TSR_ANSI_MODES) {
            *m = match w {
                // DCSM as it is set, which DECRQM answers as DATA while
                // BDSM is set (`crate::component`).
                9 => match self.cold.dcsm {
                    DeviceComponent::Data => TsrMode::Set,
                    DeviceComponent::Presentation => TsrMode::Reset,
                },
                _ => tsr_mode(self.mode_state(Marker::Standard, Param::from_raw(w))),
            };
        }
        let mut private = [TsrMode::Absent; TSR_PRIVATE_MODES];
        for (m, w) in private.iter_mut().zip(SLOT_WIRE) {
            *m = tsr_mode(self.private_state(PrivateMode::from_wire(Some(Param::from_raw(w)))));
        }
        let mut modify = [0i8; 7];
        let mut format = [0u8; 7];
        for (i, r) in KeyResource::ALL.into_iter().enumerate() {
            modify[i] = self.cold.keys.modify(r).get();
            format[i] = self.cold.keys.format(r).code();
        }
        let mask = self.cold.keys.ignore();
        let mut title_modes = 0;
        for m in self.strs.title_modes.set() {
            title_modes |= 1 << m.code();
        }
        let saved = self.cold.saved.map(|s| TsrSaved {
            row: u16::try_from(s.cy.index()).unwrap_or(u16::MAX),
            col: u16::try_from(s.cx.index()).unwrap_or(u16::MAX),
            rendition: s.style_raw,
            protect: ModeChange::of(s.protect.is_protected()),
            origin: s.origin,
            pending: s.pending,
            gl: s.gl,
            gr: s.gr,
            designations: s.desig,
        });
        let cols = self.cols.index().min(self.tabs.len());
        TerminalState {
            level: self.cold.level,
            c1: self.cold.replies.c1(),
            encoding: self.cold.encoding,
            ansi,
            private,
            rendition: self.kept_rendition(),
            top: self.cold.top.one_based().get(),
            bottom: self.cold.bot.one_based().get(),
            left: self.cold.left.one_based().get(),
            right: self.cold.right.one_based().get(),
            cursor_style: self.cold.cursor_style.wire(),
            warning: self.cold.bell.warning.reported(),
            margin: self.cold.bell.margin.reported(),
            extent: match self.cold.attr_extent {
                AttrExtent::Stream => 0,
                AttrExtent::Rectangle => 2,
            },
            status_type: self.strs.status.kind.code(),
            active: self.strs.status.active.code(),
            columns: match PageWidth::page_of(self.cols) {
                PageWidth::Eighty => 80,
                PageWidth::OneThirtyTwo => 132,
            },
            lines: u16::try_from(self.rows.index()).unwrap_or(u16::MAX).max(24),
            title_modes,
            modify,
            format,
            mask: match mask.is_none() {
                true => 0,
                false => mask.wire() - 1,
            },
            cursor: self.cursor_information(),
            saved,
            tabs: tsr_tabs(&self.tabs[..cols]),
        }
    }

    /// DECTSR, `DCS 1 $ s D...D ST`, in the controls `S7C1T`/`S8C1T` chose,
    /// whole or not at all: at most 256 characters, STD 070's bound.
    fn report_terminal_state(&mut self) {
        let mut data = [0u8; TSR_DATA_MAX + 2];
        let n = self.terminal_state().encode(&mut data);
        let c1 = self.cold.replies.c1();
        self.push_reply(&[c1.dcs(), b"1$s", &data[..n], c1.st()]);
    }

    /// DECCTR, `DCS 2 $ s Pc;Pu;Px;Py;Pz/... ST`: a group for each colour
    /// the core knows, the application's `OSC 4` override else the
    /// embedder's table, as `OSC 4`'s query answers; an entry neither gave
    /// is left out, as that query answers nothing for it. Sent whole or
    /// not at all.
    fn report_color_table(&mut self, space: ColorSpace) {
        let c1 = self.cold.replies.c1();
        let mut out = [0u8; CTR_REPLY_MAX];
        let mut n = 0;
        for part in [c1.dcs(), b"2$s"] {
            out[n..n + part.len()].copy_from_slice(part);
            n += part.len();
        }
        let mut first = true;
        for pc in 0..=255u8 {
            let i = usize::from(pc);
            let own = self.strs.palette[i];
            let word = match own.is_default() {
                true => self.strs.palette[crate::storage::EMBEDDER_PALETTE + i],
                false => own,
            };
            let Some(rgb) = word.as_rgb() else {
                continue;
            };
            if !first {
                out[n] = b'/';
                n += 1;
            }
            first = false;
            n += color_group(pc, rgb, space, &mut out[n..]);
        }
        let st = c1.st();
        out[n..n + st.len()].copy_from_slice(st);
        n += st.len();
        self.push_reply(&[&out[..n]]);
    }

    /// DECRSTS, `DCS Ps $ p`: 1 restores a DECTSR, kept to its terminator
    /// and checksummed there; 2 a DECCTR, each group set as it arrives; 0,
    /// "illegal, restore ignored", and any other `Ps` consumed and counted.
    #[cold]
    #[inline(never)]
    pub(crate) fn restore_state_start(&mut self, csi: &Csi) -> StrKind {
        match csi.params().raw(ParamIdx::FIRST, 0) {
            1 => StrKind::RestoreTerminal,
            2 => {
                self.strs.color_restore = ColorRestore::START;
                StrKind::RestoreColors
            }
            _ => StrKind::Other,
        }
    }

    /// One byte of a colour table restore's body.
    pub(crate) fn restore_color_put(&mut self, b: StreamByte) {
        let (next, entry) = self.strs.color_restore.put(b.raw_byte());
        self.strs.color_restore = next;
        if let Some(e) = entry {
            self.restore_color(e);
        }
    }

    /// The end of a colour table restore: its last group, unless the body
    /// ended on a `/`.
    pub(crate) fn restore_color_end(&mut self) {
        let last = self.strs.color_restore;
        self.strs.color_restore = ColorRestore::Stopped;
        if !last.is_fresh() {
            if let Some(e) = last.end() {
                self.restore_color(e);
            }
        }
    }

    /// One colour set, as `OSC 4` sets it: "only the index entries
    /// specified are changed" (p. 5-109).
    fn restore_color(&mut self, e: ColorEntry) {
        let (r, g, b) = e.rgb;
        self.set_palette(
            Chan::from_u8(e.index),
            ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)),
        );
    }

    /// The end of a terminal state restore: the body checksummed, then each
    /// field restored through its control, in the order the module says.
    #[cold]
    #[inline(never)]
    pub(crate) fn restore_terminal_state(&mut self, node: &mut crate::graph::Node, at: BytePos) {
        let n = self.cold.str_len.index().min(self.strs.pending.len());
        if n > TSR_DATA_MAX + 2 {
            return;
        }
        let mut body = [0u8; TSR_DATA_MAX + 2];
        body[..n].copy_from_slice(&self.strs.pending[..n]);
        let Some(r) = TerminalRestore::parse(&body[..n]) else {
            return;
        };
        let now = self.terminal_state();
        // The level first: DECSCL soft-resets.
        if let Some((level, c1)) = r.level {
            if level != now.level {
                let pc = match c1 {
                    C1Transmit::EightBit => 0,
                    C1Transmit::SevenBit => 1,
                };
                let mut c = Ctl::new();
                c.num(level.param()).push(b";").num(pc).push(b"\"p");
                self.replay(c.bytes(), node, at);
            } else if c1 != now.c1 {
                self.select_c1(c1);
            }
        }
        // `ESC % @` and `ESC % G` set only the encoding, which the parse
        // loop's live copy takes up from here when the string ends.
        if let Some(e) = r.encoding {
            let was = self.cold.strips();
            self.cold.encoding = e;
            self.reselect();
            self.restrip_from(was, node);
        }
        let now = self.terminal_state();
        for ((want, have), w) in r.ansi.iter().zip(now.ansi).zip(TSR_ANSI_MODES) {
            if let Some(m) = differs_mode(*want, have) {
                let mut c = Ctl::new();
                c.num(w).push(m);
                self.replay(c.bytes(), node, at);
            }
        }
        // The two modes that gate others go first: `?40` lets `?3` move,
        // and `?1046` lets the alternate screen be switched to.
        let gates = |w: u16| matches!(w, 40 | 1046);
        let first = r.private.iter().zip(SLOT_WIRE).filter(|(_, w)| gates(*w));
        let rest = r.private.iter().zip(SLOT_WIRE).filter(|(_, w)| !gates(*w));
        for (want, w) in first.chain(rest) {
            let have =
                tsr_mode(self.private_state(PrivateMode::from_wire(Some(Param::from_raw(w)))));
            if let Some(m) = differs_mode(*want, have) {
                let mut c = Ctl::new();
                c.push(b"?").num(w).push(m);
                self.replay(c.bytes(), node, at);
            }
        }
        let now = self.terminal_state();
        // The display first, so what follows lands on the one reported.
        if let Some(t) = r.status_type.filter(|t| *t != now.status_type) {
            let mut c = Ctl::new();
            c.num(t).push(b"$~");
            self.replay(c.bytes(), node, at);
        }
        if let Some(a) = r.active.filter(|a| *a != now.active) {
            let mut c = Ctl::new();
            c.num(a).push(b"$}");
            self.replay(c.bytes(), node, at);
        }
        let now = self.terminal_state();
        if let (Some(t), Some(b)) = (r.top, r.bottom) {
            if (t, b) != (now.top, now.bottom) {
                let mut c = Ctl::new();
                c.num(t).push(b";").num(b).push(b"r");
                self.replay(c.bytes(), node, at);
            }
        }
        // `CSI s` is DECSLRM only while `?69` is set, and SCOSC otherwise.
        if let (Some(l), Some(rt)) = (r.left, r.right) {
            if (l, rt) != (now.left, now.right) && self.cold.modes.is_set(ModeBit::SideMargins) {
                let mut c = Ctl::new();
                c.num(l).push(b";").num(rt).push(b"s");
                self.replay(c.bytes(), node, at);
            }
        }
        let now = self.terminal_state();
        let settings: [(Option<i32>, i32, &[u8]); 6] = [
            (
                r.cursor_style.map(i32::from),
                i32::from(now.cursor_style),
                b" q",
            ),
            (r.warning.map(i32::from), i32::from(now.warning), b" t"),
            (r.margin.map(i32::from), i32::from(now.margin), b" u"),
            (r.extent.map(i32::from), i32::from(now.extent), b"*x"),
            (r.columns.map(i32::from), i32::from(now.columns), b"$|"),
            (r.lines.map(i32::from), i32::from(now.lines), b"t"),
        ];
        for (want, have, fin) in settings {
            if let Some(v) = want.filter(|v| *v != have) {
                let mut c = Ctl::new();
                c.num(v).push(fin);
                self.replay(c.bytes(), node, at);
            }
        }
        if let Some(t) = r.title_modes {
            for mode in 0..4u8 {
                let (want, have) = (t & (1 << mode) != 0, now.title_modes & (1 << mode) != 0);
                if want != have {
                    let mut c = Ctl::new();
                    c.push(b">").num(mode).push(match want {
                        true => b"t",
                        false => b"T",
                    });
                    self.replay(c.bytes(), node, at);
                }
            }
        }
        for (i, r_) in KeyResource::ALL.into_iter().enumerate() {
            let pp = i32::from(r_.pp());
            let mask = match r_ {
                KeyResource::Other => r.mask,
                _ => Some(0),
            };
            if let (Some(v), Some(m)) = (r.modify[i], mask) {
                let now_mask = match r_ {
                    KeyResource::Other => now.mask,
                    _ => 0,
                };
                if (v, m) != (now.modify[i], now_mask) {
                    let mut c = Ctl::new();
                    match v {
                        -1 => c.push(b">").num(pp).push(b"n"),
                        _ if m != 0 => c
                            .push(b">")
                            .num(pp)
                            .push(b";")
                            .num(v)
                            .push(b":")
                            .num(m)
                            .push(b"m"),
                        _ => c.push(b">").num(pp).push(b";").num(v).push(b"m"),
                    };
                    self.replay(c.bytes(), node, at);
                }
            }
            if let Some(f) = r.format[i].filter(|f| *f != now.format[i]) {
                let mut c = Ctl::new();
                c.push(b">").num(pp).push(b";").num(f).push(b"f");
                self.replay(c.bytes(), node, at);
            }
        }
        if let Some(w) = r.rendition.filter(|w| *w != self.style_raw) {
            let (bytes, len) = crate::dcs::sgr_control(w);
            self.replay(&bytes[..len.index()], node, at);
        }
        if let Some((bytes, len)) = r.cursor {
            let mut now = [0u8; CIR_DATA_MAX];
            let n = self.cursor_information().encode(&mut now);
            if bytes[..usize::from(len)] != now[..n] {
                self.restore_cursor_information(&bytes[..usize::from(len)]);
            }
        }
        if let Some(t) = r.tabs {
            self.restore_tabs(t);
        }
        if let Some(s) = r.saved {
            self.restore_saved(s);
        }
    }

    /// The shown screen's `DECSC` slot as the report holds it, what `ESC 7`
    /// would have saved: emptied when the report had none, and left as it
    /// is for a save outside the screen, which baud cannot take.
    fn restore_saved(&mut self, s: Option<TsrSaved>) {
        let Some(s) = s else {
            self.cold.saved = None;
            return;
        };
        let (row, col) = (usize::from(s.row), usize::from(s.col));
        if row >= self.rows.index() || col > self.cols.index() {
            return;
        }
        let g = s.designations.map(|d| d.charset());
        let protect = match s.protect {
            ModeChange::Set => CellFlags::PROTECTED,
            ModeChange::Reset => CellFlags::NONE,
        };
        // The flags the write path stamps, as `refresh_flags` makes them
        // from the set GL invokes.
        let charset = g[s.gl.index()];
        let base = match charset {
            Charset::DecGraphics => protect.with(CellFlags::GRAPHICS),
            _ => protect,
        };
        let flags = base
            .with_received(Received::of(charset))
            .with_link(self.link);
        self.cold.saved = Some(crate::term::Saved {
            cy: Row::from_index(row),
            cx: Col::at_index(col),
            style_raw: s.rendition,
            flags,
            g,
            gl: s.gl,
            gr: s.gr,
            desig: s.designations,
            protect,
            origin: s.origin,
            pending: s.pending,
            widths: self.cold.widths.widths(),
        });
    }

    /// The tab stops a report carried, replacing every stop: a bitmap's
    /// columns, which must fit the screen, or every eighth column.
    fn restore_tabs(&mut self, t: TsrTabs) {
        let cols = self.cols.index().min(self.tabs.len());
        if t != TsrTabs::Absent {
            self.strs.aligned.all_plain();
        }
        match t {
            TsrTabs::Absent => {}
            TsrTabs::EveryEighth => {
                for (i, s) in self.tabs[..cols].iter_mut().enumerate() {
                    *s = i > 0 && i % 8 == 0;
                }
            }
            TsrTabs::Stops { cols: have, bits } => {
                let have = usize::from(have);
                let past = (cols..have).any(|i| bits[i / 8] & (1 << (i % 8)) != 0);
                if past {
                    return;
                }
                for (i, s) in self.tabs[..cols].iter_mut().enumerate() {
                    *s = i < have && bits[i / 8] & (1 << (i % 8)) != 0;
                }
            }
        }
    }

    /// One control replayed through the dispatch a parsed one takes.
    fn replay(&mut self, bytes: &[u8], node: &mut crate::graph::Node, at: BytePos) {
        crate::write::replay(self, bytes, node, at);
    }
}

/// The DECSET or SM final that moves a mode to what the report says, when
/// the report carries it and it is not already so.
fn differs_mode(want: Option<TsrMode>, have: TsrMode) -> Option<&'static [u8]> {
    match (want?, have) {
        (TsrMode::Set, TsrMode::Reset) => Some(b"h"),
        (TsrMode::Reset, TsrMode::Set) => Some(b"l"),
        _ => None,
    }
}
