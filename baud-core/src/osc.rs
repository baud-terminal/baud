use crate::*;

/// Trim a trailing NUL from an OSC string.
///
/// Some emitters terminate the payload with a NUL before the real terminator.
/// Keeping it would put a stray byte in every title.
pub(crate) fn clip(s: &[u8]) -> &[u8] {
    match s.last() {
        Some(0) => &s[..s.len() - 1],
        _ => s,
    }
}

impl<'a> Term<'a> {
    /// Append one byte to the OSC payload, truncating rather than growing.
    #[inline(always)]
    pub(crate) fn osc_push(&mut self, b: StreamByte) {
        let n = self.osc_len.index();
        if n < OSC_MAX {
            self.s.osc[n] = b.raw_byte();
            self.osc_len = ByteLen::from_index(n + 1);
        } else {
            self.osc_truncations.inc();
        }
    }

    pub(crate) fn osc_reset(&mut self) {
        self.osc_len = ByteLen::ZERO;
    }

    /// Copy a bounded string into one of the two title slots.
    pub(crate) fn set_string(dst: &mut [u8; OSC_MAX], src: &[u8]) -> ByteLen {
        let n = src.len().min(OSC_MAX);
        dst[..n].copy_from_slice(&src[..n]);
        ByteLen::from_index(n)
    }

    /// Act on a completed OSC payload.
    pub(crate) fn dispatch_osc(&mut self) {
        let n = self.osc_len.index();
        // Split the borrow: `Osc::parse` borrows the payload while the setters
        // want `&mut self.s`. Resolve to an owned decision first.
        let parsed = Osc::parse(&self.s.osc[..n]);
        match parsed {
            Osc::SetTitle(t) => {
                let t = clip(t);
                self.title_len = Self::set_string(&mut self.s.title, t);
            }
            Osc::SetIcon(t) => {
                let t = clip(t);
                self.icon_len = Self::set_string(&mut self.s.icon, t);
            }
            Osc::SetIconAndTitle(t) => {
                let t = clip(t);
                self.title_len = Self::set_string(&mut self.s.title, t);
                self.icon_len = Self::set_string(&mut self.s.icon, t);
            }
            Osc::SetColor { index, spec } => match spec {
                XColor::Rgb(r, g, b) => self.s.palette[index.index()] = ColorWord::rgb(r, g, b),
                XColor::Query => self.report_color(index),
                // A colour *name*. Not resolved, because resolving it needs
                // X11's rgb.txt and baud has no citable copy -- approximating
                // it would be guessing at a protocol. Left alone rather than
                // set to something plausible-but-wrong.
                XColor::Unresolved(_) => {}
            },
            Osc::ResetColor(Some(c)) => self.s.palette[c.index()] = ColorWord::DEFAULT,
            Osc::ResetColor(None) => self.s.palette = [ColorWord::DEFAULT; 256],
            // `OSC 52` write is accepted by the protocol but baud has no
            // clipboard to write to, and a *read* is refused on purpose --
            // see `ClipboardOp::Read`. Neither gets a reply: answering a
            // clipboard query with anything, including an empty string, tells
            // the application the feature works.
            Osc::Hyperlink { id, uri } => {
                // The payload borrows `self.s`, so resolve the slot and the
                // length before touching the table.
                let n = uri.len().min(LINK_MAX);
                if n == 0 {
                    self.link = LinkId::NONE;
                } else {
                    let id = LinkId::bucket(id, uri, NLINK);
                    let mut tmp = [0u8; LINK_MAX];
                    tmp[..n].copy_from_slice(&uri[..n]);
                    self.s.links[id.index()] = tmp;
                    self.s.link_len[id.index()] = ByteLen::from_index(n);
                    self.link = id;
                }
                self.refresh_flags();
            }
            Osc::Clipboard(_) => {}
            Osc::Unsupported => self.unsupported.inc(),
        }
        self.osc_reset();
    }

    /// How `OSC 8` opens and closes a link.
    ///
    /// An empty URI closes: the [explicit hyperlink spec][osc8] ends a run with
    /// `OSC 8 ; ; ST`, and treating that as "a link to nowhere" would make
    /// every cell after it clickable.
    ///
    /// The table is bounded and **evicts on collision**, exactly like the style
    /// table. A URI is attacker-supplied content used as a key, which
    /// `CLAUDE.md` names as the content-addressing failure mode; a page cycling
    /// through thousands of links must cost the same as one with four. An
    /// evicted link renders as an ordinary cell, the same tolerable degradation
    /// an evicted style has.
    ///
    /// The dispatch is inline in `dispatch_osc` because the URI borrows the
    /// payload buffer that lives in `self.s`.
    ///
    /// [osc8]: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda
    /// The URI a cell links to, if any.
    pub fn link_uri(&self, l: LinkId) -> Option<&[u8]> {
        if l.is_none() {
            return None;
        }
        let n = self.s.link_len[l.index()].index();
        (n > 0).then(|| &self.s.links[l.index()][..n])
    }

    /// Reply to `OSC 4 ; c ; ?` in the same shape as the setter.
    ///
    /// `[[source: ctlseqs]]`: "xterm replies with a control sequence of the
    /// same form which can be used to set the corresponding color", and it
    /// "uses the same terminator used in a query".
    pub(crate) fn report_color(&mut self, index: Chan) {
        let c = self.s.palette[index.index()];
        let Some((r, g, b)) = c.as_rgb() else {
            // Not overridden. baud has no built-in palette table to report --
            // the renderer owns that -- so it says nothing rather than
            // inventing a colour the renderer does not use.
            return;
        };
        let term = self.osc_term;
        self.emit(b"\x1b]4;");
        self.emit_dec(index.get());
        self.emit(b";rgb:");
        self.emit_hex(r);
        self.emit(b"/");
        self.emit_hex(g);
        self.emit(b"/");
        self.emit_hex(b);
        match term {
            StringTerm::St => self.emit(b"\x1b\\"),
            StringTerm::Bel => self.emit(b"\x07"),
        }
    }

    /// The window title, as the application set it. Bounded by `OSC_MAX`.
    pub fn title(&self) -> &[u8] {
        &self.s.title[..self.title_len.index()]
    }

    /// The icon name. `OSC 1`, and `OSC 0` sets both.
    pub fn icon_name(&self) -> &[u8] {
        &self.s.icon[..self.icon_len.index()]
    }

    /// A palette override from `OSC 4`, if the application set one.
    ///
    /// `None` means "use your built-in colour": the core deliberately holds no
    /// default palette, because the mapping from index to pixels belongs to the
    /// renderer and a second copy here would be one to keep in step.
    pub fn palette(&self, index: Chan) -> Option<(u8, u8, u8)> {
        self.s.palette[index.index()].as_rgb()
    }
}
