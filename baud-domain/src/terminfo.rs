//! terminfo's padding, `$<..>`, and the one capability baud reads it from.
//!
//! **The number is not on the wire.** `tput flash` under `xterm-256color`
//! sends `\E[?5h`, waits the `$<100/>` the entry names, and sends `\E[?5l`:
//! the delay is performed by the sender (`tputs`, per `[[source:
//! terminfo-man5]]`), so a terminal sees two sequences and no length. On
//! macOS `tput` buffers across the wait and both halves arrive in one read
//! (a pty capture, 2026-09-25). So a terminal that wants to show the flash for
//! as long as the application meant has to read the length from the entry it
//! advertised as `$TERM` -- which is a pinned file here, read at build time.
//!
//! **The grammar** (`[[source: terminfo-man5]]`, "A delay in milliseconds may
//! appear anywhere in a string capability"): a number "with at most one
//! decimal place of precision", followed by `*` (per line affected), `/`
//! (mandatory, even under `xon`), both, or neither. What the text does not
//! settle is refused rather than guessed: a second decimal place, a point
//! with no digit after it, an empty number, a suffix given twice.
//!
//! **What the types bound, and what they do not.** [`Delay`] is tenths of a
//! millisecond -- the grammar's own precision -- in a `u16`, so it holds up to
//! 6553.5 ms and a longer delay is refused rather than wrapped. The terminfo
//! text sets no maximum and none is invented here. That the *file* is the one
//! pinned, byte for byte, is `scripts/check-sources.sh`'s job against the
//! manifest's SHA-256; these types say the bytes mean a flash of one length.

use core::time::Duration;

/// A padding length: tenths of a millisecond, the one decimal place the
/// grammar allows.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Delay(u16);

impl Delay {
    /// The length as a `Duration`, for a deadline.
    pub const fn duration(self) -> Duration {
        // A tenth of a millisecond is 100 us.
        Duration::from_micros(self.0 as u64 * 100)
    }

    /// Whole milliseconds and the tenth, the way the entry wrote it: `100`
    /// and `0` for `$<100>`, `2` and `5` for `$<2.5>`.
    pub const fn millis_and_tenth(self) -> (u16, u16) {
        (self.0 / 10, self.0 % 10)
    }
}

/// `*`: whether the delay is per line the operation affects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Per {
    /// No `*`: the delay is taken once.
    Once,
    /// `*`: the delay is multiplied by the lines affected.
    Line,
}

/// `/`: whether the delay is taken on a terminal that has `xon`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Force {
    /// No `/`: "padding is advisory if the device has the `xon`
    /// capability".
    Advisory,
    /// `/`: "forces a delay of the given number of milliseconds even on
    /// devices for which `xon` is present".
    Mandatory,
}

/// One `$<..>`: its length and its two suffixes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Padding {
    pub delay: Delay,
    pub per: Per,
    pub force: Force,
}

/// Why a `$<..>` is not one the grammar admits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaddingError {
    /// No digit at all: `$<>`, `$<*>`.
    Empty,
    /// A byte that is not a digit, a point or a suffix.
    Byte,
    /// More than one decimal place: `$<1.25>`.
    Precision,
    /// A point with no digit after it: `$<5.>`.
    Point,
    /// A suffix given twice: `$<5**>`.
    Suffix,
    /// More than a `u16` of tenths holds: past 6553.5 ms.
    Overflow,
}

impl Padding {
    /// The text between `$<` and `>`.
    pub const fn parse(text: &[u8]) -> Result<Padding, PaddingError> {
        let mut i = 0;
        // Whole milliseconds first, bounded as they come so no digit count
        // can overflow the accumulator; the tenths are the bound that counts.
        let mut millis: u32 = 0;
        let mut digits = 0;
        while i < text.len() && text[i].is_ascii_digit() {
            millis = millis * 10 + (text[i] - b'0') as u32;
            if millis > u16::MAX as u32 {
                return Err(PaddingError::Overflow);
            }
            digits += 1;
            i += 1;
        }
        let mut tenths = millis * 10;
        // `.5` is a number with no whole part, and is admitted: the grammar
        // bounds the decimal places, not the digits before the point.
        if i < text.len() && text[i] == b'.' {
            i += 1;
            if i >= text.len() || !text[i].is_ascii_digit() {
                return Err(PaddingError::Point);
            }
            tenths += (text[i] - b'0') as u32;
            digits += 1;
            i += 1;
            if i < text.len() && text[i].is_ascii_digit() {
                return Err(PaddingError::Precision);
            }
        }
        if digits == 0 {
            return Err(PaddingError::Empty);
        }
        if tenths > u16::MAX as u32 {
            return Err(PaddingError::Overflow);
        }
        let mut per = Per::Once;
        let mut force = Force::Advisory;
        while i < text.len() {
            match text[i] {
                b'*' => match per {
                    Per::Once => per = Per::Line,
                    Per::Line => return Err(PaddingError::Suffix),
                },
                b'/' => match force {
                    Force::Advisory => force = Force::Mandatory,
                    Force::Mandatory => return Err(PaddingError::Suffix),
                },
                _ => return Err(PaddingError::Byte),
            }
            i += 1;
        }
        Ok(Padding {
            delay: Delay(tenths as u16),
            per,
            force,
        })
    }
}

/// A string capability's value in a terminfo *source* entry (`infocmp`'s
/// form, what the pinned `.ti` files are), still escaped. `None` when the
/// entry does not name it, or cancels it with `name@`.
///
/// A field ends at the first `,` not escaped. `\` escapes the byte after it
/// and `^x` is one control character, so neither is read as a terminator --
/// except that `^` straight after `%` is the parameter language's
/// exclusive-OR, `%^` (`[[source: terminfo-man5]]`'s bit operations), and
/// escapes nothing. A line whose first byte is `#` is a comment.
/// The first field is the entry's names, which `|` separates and no
/// capability is.
pub const fn string_capability<'a>(entry: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let mut i = 0;
    let mut field = 0;
    let mut start = 0;
    let mut line_start = true;
    // What the last unit read stood for: `^%` is control-%, so a `^` after it
    // is an escape again.
    let mut last = 0;
    while i < entry.len() {
        let b = entry[i];
        if line_start && b == b'#' {
            while i < entry.len() && entry[i] != b'\n' {
                i += 1;
            }
            // The newline itself is whitespace, and the next line starts.
            start = i + 1;
            i += 1;
            continue;
        }
        line_start = b == b'\n';
        match b {
            b'\\' => {
                last = if i + 1 < entry.len() { entry[i + 1] } else { 0 };
                i += 2;
            }
            b'^' if last != b'%' => {
                last = if i + 1 < entry.len() {
                    entry[i + 1] & 0x1f
                } else {
                    0
                };
                i += 2;
            }
            b',' => {
                last = b;
                if field > 0 {
                    if let Some(value) = field_value(entry, start, i, name) {
                        return Some(value);
                    }
                }
                field += 1;
                i += 1;
                start = i;
            }
            _ => {
                last = b;
                i += 1;
            }
        }
    }
    None
}

/// The value of the field `entry[start..end]` if its name is `name` and it
/// has one: `name=value`. Leading whitespace is skipped.
const fn field_value<'a>(
    entry: &'a [u8],
    start: usize,
    end: usize,
    name: &[u8],
) -> Option<&'a [u8]> {
    let mut s = start;
    while s < end && entry[s].is_ascii_whitespace() {
        s += 1;
    }
    if end - s < name.len() + 1 {
        return None;
    }
    let mut k = 0;
    while k < name.len() {
        if entry[s + k] != name[k] {
            return None;
        }
        k += 1;
    }
    if entry[s + name.len()] != b'=' {
        return None;
    }
    let (_, value) = entry.split_at(s + name.len() + 1);
    let (value, _) = value.split_at(end - (s + name.len() + 1));
    Some(value)
}

/// What an entry's `flash` is, read as the protocol it drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flash {
    /// The entry names no `flash` (or cancels it): the application has no
    /// visual bell to send.
    Absent,
    /// `\E[?5h\E[?5l`, with no padding between: the entry does not say how
    /// long the reversed screen should show.
    Unpadded,
    /// `\E[?5h$<..>\E[?5l`: reverse video, held for the padding, and back.
    Padded(Padding),
}

/// Why an entry's `flash` is not a reversed screen held for a length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlashError {
    /// Not `\E[?5h`, one optional padding, `\E[?5l`: a flash that sets
    /// another mode, or pads outside the reversed screen, has no length that
    /// means "how long the screen stays reversed".
    Shape,
    /// A `$<` with no `>`.
    Unterminated,
    /// The padding is not one the grammar admits.
    Padding(PaddingError),
    /// `*`: a flash affects no line, so a per-line delay has no length.
    PerLine,
}

/// `\E[?5h`, decoded: DECSCNM set.
const SET: &[u8] = b"\x1b[?5h";
/// `\E[?5l`, decoded: DECSCNM reset.
const RESET: &[u8] = b"\x1b[?5l";

/// Read an entry's `flash` as DECSCNM set, held and reset: the set, at most
/// one padding, the reset, and nothing else, compared after the source
/// escapes are decoded -- so `\E` and `\e` are one byte, as terminfo(5)
/// says, and a padding placed outside the reversed screen is no length.
pub const fn flash(entry: &[u8]) -> Result<Flash, FlashError> {
    let value = match string_capability(entry, b"flash") {
        Some(v) => v,
        None => return Ok(Flash::Absent),
    };
    let mut i = match expect(value, 0, SET) {
        Some(i) => i,
        None => return Err(FlashError::Shape),
    };
    let mut padding = None;
    if i + 1 < value.len() && value[i] == b'$' && value[i + 1] == b'<' {
        let open = i + 2;
        let mut close = open;
        while close < value.len() && value[close] != b'>' {
            close += 1;
        }
        if close == value.len() {
            return Err(FlashError::Unterminated);
        }
        let (_, text) = value.split_at(open);
        let (text, _) = text.split_at(close - open);
        padding = match Padding::parse(text) {
            Ok(p) => match p.per {
                Per::Once => Some(p),
                Per::Line => return Err(FlashError::PerLine),
            },
            Err(e) => return Err(FlashError::Padding(e)),
        };
        i = close + 1;
    }
    i = match expect(value, i, RESET) {
        Some(i) => i,
        None => return Err(FlashError::Shape),
    };
    if i != value.len() {
        return Err(FlashError::Shape);
    }
    match padding {
        Some(p) => Ok(Flash::Padded(p)),
        None => Ok(Flash::Unpadded),
    }
}

/// Where `value` continues after it decodes to `want` from `at`, or `None`.
const fn expect(value: &[u8], at: usize, want: &[u8]) -> Option<usize> {
    let mut i = at;
    let mut k = 0;
    while k < want.len() {
        let (b, next) = match decode(value, i) {
            Some(d) => d,
            None => return None,
        };
        if b != want[k] {
            return None;
        }
        i = next;
        k += 1;
    }
    Some(i)
}

/// One character of an escaped string capability, and where the next
/// starts: terminfo(5)'s escapes. `\E` and `\e` are ESC; `^x` is control-x;
/// `^?` is read as DEL, which the pinned page does not name -- control-`?`
/// by its rule would be 0x1f, and nothing a terminal description means; `\n
/// \l \r \t \b \f \s` are newline, line feed,
/// return, tab, backspace, form feed and space; `\^ \\ \, \:` are the
/// byte itself; `\0` is 0x80 ("which does not terminate a string"); and a
/// backslash before three octal digits is that byte. `None` past the end or
/// on an escape the text does not name.
const fn decode(value: &[u8], i: usize) -> Option<(u8, usize)> {
    if i >= value.len() {
        return None;
    }
    match value[i] {
        b'^' => {
            if i + 1 >= value.len() {
                return None;
            }
            match value[i + 1] {
                b'?' => Some((0x7f, i + 2)),
                c => Some((c & 0x1f, i + 2)),
            }
        }
        b'\\' => {
            if i + 1 >= value.len() {
                return None;
            }
            let c = value[i + 1];
            if octal(value, i + 1) {
                let n =
                    (value[i + 1] - b'0') * 64 + (value[i + 2] - b'0') * 8 + (value[i + 3] - b'0');
                return Some((n, i + 4));
            }
            let b = match c {
                b'E' | b'e' => 0x1b,
                b'n' | b'l' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'b' => 0x08,
                b'f' => 0x0c,
                b's' => b' ',
                b'^' | b'\\' | b',' | b':' => c,
                b'0' => 0x80,
                _ => return None,
            };
            Some((b, i + 2))
        }
        b => Some((b, i + 1)),
    }
}

/// Three octal digits at `at`, the first at most `3` so the byte fits.
const fn octal(value: &[u8], at: usize) -> bool {
    at + 2 < value.len()
        && value[at] >= b'0'
        && value[at] <= b'3'
        && value[at + 1] >= b'0'
        && value[at + 1] <= b'7'
        && value[at + 2] >= b'0'
        && value[at + 2] <= b'7'
}
