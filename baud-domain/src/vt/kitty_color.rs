//! kitty's colour control, `OSC 21 ; key=value ; key=value ; ... ST`
//! (`[[source: kitty-6d5d0c4-color-stack]]`, "Setting and querying
//! colors"): one record of it read, and the colour value its text defines.
//!
//! **A record is read as kitty's color_control reads it**
//! (`[[source: kitty-window-py]]` 571-641): split at its first `=`; with
//! none it resets the key, "the value it would have if it was never set";
//! a value of `?` asks for it; an empty value sets "a dynamic color"; any
//! other value is a colour. The key is "a number from 0-255", or one of the
//! names of the text's table.
//!
//! **A value is read by the text's grammar, not by kitty's parser**, where
//! the two differ (Jev's lean, G_DOC, 0.50/0.74 and 0.44/0.74 after
//! research, under the bar): `rgb:` scaled, `#` as the high bits, `rgbi:`
//! clipped to 0 to 1, kitty's colour names, and an optional `@` alpha in the
//! same float grammar. kitty's Color.parse_color reads `rgb:` by its first
//! two digits a channel, refuses every `rgbi:` (it tests the byte after the
//! `i` for a second `i`, `[[source: kitty-colors-1e1486e]]` 1532-1536),
//! and takes `oklch(`, `lab(` and a trailing `#` comment the text does not
//! name: its departures.

use super::osc::{device, high, sharp};
use crate::Chan;

/// One of the colours kitty's table names (`[[source:
/// kitty-6d5d0c4-color-stack]]` 62-77). `transparent_background_color1`
/// to `7` are not here: baud draws no background at an opacity of its own
/// colour, so they are keys it does not know ([`KittyTarget::Unknown`]),
/// answered as kitty answers one (Jev, T_UNKNOWN_ROW, 0.98 both orders).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyKey {
    Foreground,
    Background,
    SelectionBackground,
    SelectionForeground,
    Cursor,
    CursorText,
    VisualBell,
}

impl KittyKey {
    fn of(name: &[u8]) -> Option<KittyKey> {
        Some(match name {
            b"foreground" => KittyKey::Foreground,
            b"background" => KittyKey::Background,
            b"selection_background" => KittyKey::SelectionBackground,
            b"selection_foreground" => KittyKey::SelectionForeground,
            b"cursor" => KittyKey::Cursor,
            b"cursor_text" => KittyKey::CursorText,
            b"visual_bell" => KittyKey::VisualBell,
            _ => return None,
        })
    }
}

/// What a record's key names: a named colour, a palette entry, or nothing
/// baud knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyTarget {
    Named(KittyKey),
    /// "a number from 0-255": decimal digits and nothing else, leading
    /// zeros allowed, as kitty's int reads `01`.
    Entry(Chan),
    Unknown,
}

impl KittyTarget {
    fn of(key: &[u8]) -> KittyTarget {
        if let Some(k) = KittyKey::of(key) {
            return KittyTarget::Named(k);
        }
        if key.is_empty() || !key.iter().all(u8::is_ascii_digit) {
            return KittyTarget::Unknown;
        }
        let n = key.iter().try_fold(0u16, |n, &d| {
            let n = n * 10 + u16::from(d - b'0');
            (n <= 255).then_some(n)
        });
        match n.and_then(|n| u8::try_from(n).ok()) {
            Some(n) => KittyTarget::Entry(Chan::from_u8(n)),
            None => KittyTarget::Unknown,
        }
    }
}

/// What a record asks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyOp {
    /// `key=?`.
    Query,
    /// `key=value`, a colour the grammar reads.
    Set(Chan, Chan, Chan),
    /// `key=value`, a value the grammar does not read.
    Unreadable,
    /// `key=`: "the terminal should use a dynamic color".
    Automatic,
    /// `key`: "reset a color to its default value".
    Reset,
}

/// One `key=value` record of an `OSC 21` body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyRecord<'a> {
    /// The key as sent, which a reply echoes.
    pub key: &'a [u8],
    pub target: KittyTarget,
    pub op: KittyOp,
}

impl<'a> KittyRecord<'a> {
    /// A record, its body's non-printable bytes already dropped; `None`
    /// for an empty one, which asks nothing (a trailing `;`).
    pub fn parse(rec: &'a [u8]) -> Option<KittyRecord<'a>> {
        if rec.is_empty() {
            return None;
        }
        let (key, op) = match rec.iter().position(|&b| b == b'=') {
            None => (rec, KittyOp::Reset),
            Some(eq) => {
                let value = &rec[eq + 1..];
                let op = match value {
                    b"?" => KittyOp::Query,
                    b"" => KittyOp::Automatic,
                    v => match kitty_spec(v) {
                        Some((r, g, b)) => KittyOp::Set(r, g, b),
                        None => KittyOp::Unreadable,
                    },
                };
                (&rec[..eq], op)
            }
        };
        Some(KittyRecord {
            key,
            target: KittyTarget::of(key),
            op,
        })
    }
}

/// A colour value as `[[source: kitty-6d5d0c4-color-stack]]` encodes one,
/// "inherited from the scheme used by XTerm ... a sane, rigorously
/// specified subset": `rgb:<red>/<green>/<blue>` of one to four hex digits
/// each, "the value scaled"; `#` with 3, 6, 9 or 12 digits, which "represent
/// the most significant bits"; `rgbi:` floats whose values "outside the 0 -
/// 1 range must be clipped"; or a colour name of kitty's, "accepted
/// (case-insensitively)". Each read to sixteen bits a channel as XParseColor
/// reads the same forms (`XColor::parse`), the high byte kept. An alpha
/// may follow, "@number between zero and one" in the float grammar; it
/// names nothing for these keys and is checked and dropped. Spaces around
/// the value are ignored, as kitty's and ghostty's readers trim them.
pub fn kitty_spec(v: &[u8]) -> Option<(Chan, Chan, Chan)> {
    let v = trim_spaces(v);
    let colour = match v.iter().position(|&b| b == b'@') {
        Some(at) => {
            super::xcms::number(&v[at + 1..])?;
            &v[..at]
        }
        None => v,
    };
    let prefixed = |p: &[u8]| {
        (colour.len() >= p.len() && colour[..p.len()].eq_ignore_ascii_case(p))
            .then(|| &colour[p.len()..])
    };
    let wide = if let Some(digits) = colour.strip_prefix(b"#") {
        sharp(digits)?
    } else if let Some(fields) = prefixed(b"rgb:") {
        device(fields)?
    } else if let Some(fields) = prefixed(b"rgbi:") {
        super::xcms::rgbi_clipped(fields)?
    } else {
        let [r, g, b] = crate::themefile::kitty_colour_name(colour)?;
        return Some((Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)));
    };
    let [r, g, b] = wide;
    Some((high(r), high(g), high(b)))
}

fn trim_spaces(v: &[u8]) -> &[u8] {
    let start = v.iter().take_while(|&&b| b == b' ').count();
    let end = v.len() - v[start..].iter().rev().take_while(|&&b| b == b' ').count();
    &v[start..end]
}

/// **The colour a palette entry past the sixteen has when nothing set
/// it**, xterm's fixed 256: the 6x6x6 cube from 16 and the 24 greys from
/// 232, as kitty's FG_BG_256 builds them (`[[source:
/// kitty-colors-1e1486e]]` 40-55). What kitty's empty value gives such an
/// entry under its default palette_generate fixed (set_color, 686-697),
/// where an entry of the sixteen goes back to its configured colour.
/// Built at compile time; entries below 16 hold black and are never read.
pub fn fixed_entry(i: Chan) -> (Chan, Chan, Chan) {
    let [r, g, b] = FIXED[usize::from(i.get())];
    (Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

const FIXED: [[u8; 3]; 256] = {
    const STEPS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    let mut t = [[0u8; 3]; 256];
    let mut i = 0;
    while i < 216 {
        t[16 + i] = [STEPS[(i / 36) % 6], STEPS[(i / 6) % 6], STEPS[i % 6]];
        i += 1;
    }
    let mut k = 0;
    while k < 24 {
        let v = 8 + k as u8 * 10;
        t[232 + k] = [v, v, v];
        k += 1;
    }
    t
};
