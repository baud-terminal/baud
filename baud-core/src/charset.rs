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

    /// The character set currently invoked into GL.
    pub fn charset(&self) -> Charset {
        self.g[self.gl.index()]
    }
}
