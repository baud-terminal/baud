use crate::*;

impl<'a> Term<'a> {
    /// `ESC ( ) * + <C>` — designate a character set into a G-slot.
    ///
    /// Designating does not change what is displayed until the slot is invoked
    /// into GL by SO/SI, which is why this only records.
    pub(crate) fn designate(&mut self, slot: GSlot, c: StreamByte) {
        self.g[slot.index()] = Charset::from_final(c);
        self.refresh_flags();
    }

    /// `ESC ( % 6` — designate a set whose final takes two bytes.
    pub(crate) fn designate_multi(&mut self, slot: GSlot, inter: StreamByte, c: StreamByte) {
        let set = Charset::from_multi(inter, c);
        if set == Charset::Other {
            // A two-byte final the specification does not list. Counted rather
            // than recorded, so it cannot look designated.
            self.unsupported.inc();
        }
        self.g[slot.index()] = set;
        self.refresh_flags();
    }

    /// The set designated into one G-slot, invoked or not.
    ///
    /// `Term::charset` reports GL only, so a designation into G1, G2 or G3 was
    /// invisible from outside -- `ESC ) 0` recorded the line-drawing set and
    /// nothing could observe that it had. A renderer needs all four, because
    /// SO/SI can invoke any of them without another designation.
    pub fn charset_in(&self, slot: GSlot) -> Charset {
        self.g[slot.index()]
    }

    /// How incoming bytes are decoded. `ESC % @` / `ESC % G` select it.
    pub fn input_encoding(&self) -> InputEncoding {
        self.encoding
    }

    /// Whether replies use 7-bit or 8-bit C1 introducers. S7C1T / S8C1T.
    pub fn c1_transmit(&self) -> C1Transmit {
        self.c1
    }

    /// The character set currently invoked into GL.
    pub fn charset(&self) -> Charset {
        self.g[self.gl.index()]
    }
}
