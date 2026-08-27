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

    /// The character set currently invoked into GL.
    pub fn charset(&self) -> Charset {
        self.g[self.gl.index()]
    }
}
