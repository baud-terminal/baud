use crate::*;

impl<'a> Term<'a> {
    /// DECSTR. Modes, region, cursor and attributes to their defaults --
    /// **not** the screen contents. That is what makes it soft, and it is why
    /// an application uses it to recover a terminal without losing scrollback.
    pub(crate) fn soft_reset(&mut self) {
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
        self.style = StyleId::DEFAULT;
        self.style_raw = StyleWord::DEFAULT;
        self.flags = CellFlags::NONE;
        self.protect = CellFlags::NONE;
        self.keypad = KeypadMode::Numeric;
        self.set_terminal_mode(TerminalMode::Ansi);
        self.mouse = MouseTracking::Off;
        self.mouse_encoding = MouseEncoding::X10;
        self.g = [Charset::Ascii; 4];
        self.gl = GSlot::G0;
        self.cursor_style = CursorStyle::BlinkingBlock;
        self.screen = Screen::Normal;
        self.alt_saved = None;
        self.saved = None;
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        self.reset_side_margins();
        self.modes = Modes::default();
    }

    /// RIS. Everything back to power-on state, including the scrolling region
    /// and the modes -- a region left over across a reset is how an application
    /// ends up drawing into a window it did not ask for.
    pub(crate) fn reset(&mut self) {
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
        self.style = StyleId::DEFAULT;
        self.style_raw = StyleWord::DEFAULT;
        self.flags = CellFlags::NONE;
        self.protect = CellFlags::NONE;
        self.keypad = KeypadMode::Numeric;
        self.set_terminal_mode(TerminalMode::Ansi);
        self.mouse = MouseTracking::Off;
        self.mouse_encoding = MouseEncoding::X10;
        self.g = [Charset::Ascii; 4];
        self.gl = GSlot::G0;
        self.cursor_style = CursorStyle::BlinkingBlock;
        self.screen = Screen::Normal;
        self.alt_saved = None;
        self.saved = None;
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        self.reset_side_margins();
        self.modes = Modes::default();
        self.reset_tabs();
        self.clear_screen();
    }
}
