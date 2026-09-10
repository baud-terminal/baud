use crate::*;

/// Which protocol the parser is speaking. DECANM, `CSI ? 2 h` / `l`.
///
/// `[[source: vt510-decanm]]`: "DECANM changes the terminal to the VT52 mode of
/// operation. In VT52 mode, the terminal acts like a VT52 terminal."
///
/// This is a **mode, not a superset**, and that is the whole reason it needs
/// its own type. The same bytes mean different things in each: `ESC D` is IND
/// (down one line) in ANSI and cursor-*left* in VT52; `ESC H` is HTS in ANSI
/// and cursor-*home* in VT52. A parser that tried to serve both from one table
/// would be wrong for one of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TerminalMode {
    #[default]
    Ansi,
    Vt52,
}

/// A VT52 escape sequence.
///
/// The set is `[[source: vt510-decanm]]`'s Table 5-1. Print functions
/// (`ESC ^ _ W X ] V`) are recognised and declined — there is no printer, and
/// accepting them silently would be the `parsed` failure.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vt52 {
    CursorUp,
    CursorDown,
    CursorRight,
    CursorLeft,
    /// `ESC F` / `ESC G`. The VT52 route into the line-drawing set.
    GraphicsOn,
    GraphicsOff,
    Home,
    /// `ESC I`. Reverse line feed.
    ReverseLineFeed,
    EraseToEndOfScreen,
    EraseToEndOfLine,
    /// `ESC Y` — followed by two bias-`0x20` coordinate bytes, so it needs the
    /// parser to collect them rather than being dispatchable on its own.
    DirectAddress,
    /// `ESC Z`. Answered with `ESC / Z`.
    Identify,
    /// `ESC <`. Leave VT52, return to ANSI.
    ExitVt52,
    KeypadApplication,
    KeypadNumeric,
    /// Recognised and deliberately not acted on.
    Unsupported,
}

impl Vt52 {
    pub fn parse(b: StreamByte) -> Vt52 {
        match b.raw_byte() {
            b'A' => Vt52::CursorUp,
            b'B' => Vt52::CursorDown,
            b'C' => Vt52::CursorRight,
            b'D' => Vt52::CursorLeft,
            b'F' => Vt52::GraphicsOn,
            b'G' => Vt52::GraphicsOff,
            b'H' => Vt52::Home,
            b'I' => Vt52::ReverseLineFeed,
            b'J' => Vt52::EraseToEndOfScreen,
            b'K' => Vt52::EraseToEndOfLine,
            b'Y' => Vt52::DirectAddress,
            b'Z' => Vt52::Identify,
            b'<' => Vt52::ExitVt52,
            b'=' => Vt52::KeypadApplication,
            b'>' => Vt52::KeypadNumeric,
            _ => Vt52::Unsupported,
        }
    }
}

/// One coordinate of `ESC Y`, biased by `0x20` on the wire.
///
/// **A source conflict, recorded rather than smoothed over.**
/// `[[source: vt510-decanm]]`'s Table 5-1 describes `ESC Y Pn` as "Move cursor
/// to column Pn" — one operand. Every implementation, and the terminfo entry
/// for `vt52`, uses **two**: `cup=\EY%p1%' '%+%c%p2%' '%+%c`, row then column,
/// each with `0x20` added. baud follows terminfo, because that is what
/// applications emit; the VT510 line appears to be an abbreviation rather than
/// a different protocol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vt52Coord(u8);

impl Vt52Coord {
    /// The origin, and what a `Cold` starts at: no `ESC Y` has been seen.
    pub const ZERO: Vt52Coord = Vt52Coord(0);

    /// Undo the wire bias. Values below `0x20` would be negative coordinates,
    /// which no application sends; they clamp to the origin rather than wrap.
    pub fn from_wire(b: StreamByte) -> Vt52Coord {
        Vt52Coord(b.raw_byte().saturating_sub(b' '))
    }
    pub fn to_row(self, rows: Rows) -> Row {
        Row::from_index((self.0 as usize).min(rows.index().saturating_sub(1)))
    }
    pub fn to_col(self, cols: Cols) -> Col {
        Col::at_index((self.0 as usize).min(cols.index().saturating_sub(1)))
    }
}
