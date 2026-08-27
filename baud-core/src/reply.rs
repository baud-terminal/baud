use crate::*;

impl<'a> Term<'a> {
    /// Fast-path hits: CSI or `ESC Fe` sequences consumed in bulk.
    ///
    /// These counters have been fed since the fast paths were written and never
    /// read anywhere, which is how "is the fast path even firing on this
    /// target?" became a question that could not be answered from the outside.
    /// Bytes the terminal owes the host, e.g. the answer to a DA or DSR.
    ///
    /// Write these back to wherever the input came from, then call
    /// [`Term::clear_reply`]. Empty when there is nothing to say.
    pub fn reply(&self) -> &[u8] {
        &self.s.reply[..self.reply_len.index()]
    }

    /// Drop the pending reply after sending it.
    pub fn clear_reply(&mut self) {
        self.reply_len = ByteLen::ZERO;
    }

    /// Append to the pending reply, or count the loss.
    pub(crate) fn emit(&mut self, bytes: &[u8]) {
        let at = self.reply_len.index();
        if at + bytes.len() > REPLY {
            self.reply_overflows.inc();
            return;
        }
        self.s.reply[at..at + bytes.len()].copy_from_slice(bytes);
        self.reply_len = ByteLen::from_index(at + bytes.len());
    }

    /// Queue a report.
    ///
    /// The only way to put bytes in the reply buffer that a caller will read as
    /// an answer. `emit` still exists for the raw append, but every *reply*
    /// goes through a named [`Report`] -- the shape is chosen by the variant,
    /// so a DSR cannot be answered with a DA's bytes.
    pub(crate) fn report(&mut self, r: Report) {
        let bytes = r.encode();
        self.emit(bytes.as_bytes());
    }

    /// The current rendition as an SGR parameter string, always starting at 0.
    ///
    /// Leading `0` is not padding: DECRQSS answers with something the
    /// application can send back verbatim, and a string that only *adds*
    /// attributes would not reproduce the state on a terminal that had others
    /// set.
    pub(crate) fn emit_sgr(&mut self, w: StyleWord) {
        self.emit(b"0");
        for (bit, code) in Attrs::WIRE {
            if w.attrs().has(bit) {
                self.emit(b";");
                self.emit_dec(code);
            }
        }
        let (fg, bg) = (w.fg(), w.bg());
        if !fg.is_default() {
            self.emit(b";");
            self.emit_colour(fg, 38);
        }
        if !bg.is_default() {
            self.emit(b";");
            self.emit_colour(bg, 48);
        }
    }

    pub(crate) fn emit_colour(&mut self, c: ColorWord, base: u8) {
        if let Some((r, g, b)) = c.as_rgb() {
            self.emit_dec(base);
            self.emit(b";2;");
            self.emit_dec(r);
            self.emit(b";");
            self.emit_dec(g);
            self.emit(b";");
            self.emit_dec(b);
        } else if let Some(i) = c.as_index() {
            self.emit_dec(base);
            self.emit(b";5;");
            self.emit_dec(i);
        }
    }

    /// A 1-based coordinate as decimal, for a DECRQSS body.
    ///
    /// Wider than [`Term::emit_dec`] because a margin can be past column 255.
    pub(crate) fn emit_num(&mut self, v: OneBased) {
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

    pub(crate) fn emit_dec(&mut self, v: u8) {
        const D: &[u8; 10] = b"0123456789";
        let mut buf = [0u8; 3];
        let mut i = buf.len();
        let mut n = v;
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

    pub(crate) fn emit_hex(&mut self, v: u8) {
        const H: &[u8; 16] = b"0123456789abcdef";
        let hi = H[usize::from(v >> 4)];
        let lo = H[usize::from(v & 0xf)];
        self.emit(&[hi, lo]);
    }
}
