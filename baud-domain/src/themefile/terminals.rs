//! Other terminals' own theme files, each read by its own grammar onto the
//! same [`Scheme`] (note Y, decision 2: "Adding a format is a new reader
//! and a new table onto the same type").
//!
//! Every reader keeps what the format's own parser keeps: a line its
//! terminal would skip is handed to `ignored` with its line and the file is
//! still read; a value the terminal reads and baud does not draw refuses the
//! file, as the ghostty reader refuses an X11 name, because reading the
//! rest would draw a theme that is not the file's. Each reads bytes, as its
//! terminal does.

use super::{
    CellPaint, CursorText, Format, Line, Scheme, SchemeError, SchemeErrorKind, SelectionText,
    Target, rgb, trim_with,
};
use crate::Color;

/// Why a reader passed a line over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skipped {
    /// A setting that is not a colour baud reads.
    NotAColour,
    /// A colour the terminal itself refuses, so it skips the line too.
    Refused,
    /// A section that holds no colours, and every line in it.
    Section,
    /// A line the terminal's grammar does not read.
    Syntax,
    /// A colour the terminal has and a scheme has no place for: alacritty's
    /// dim colours, its search and hint colours.
    Unheld,
    /// Not passed over: a colour outside sRGB, read with each channel
    /// clipped, as macOS clips it.
    Clipped,
    /// An X resource for one program (`URxvt*color0`): the key is the
    /// whole name, its first component the program.
    Program,
    /// `#include "f"` naming a file that cannot be read: the key is the
    /// name.
    NoFile,
    /// `#include "f"` nested past Xrm's 100: the key is the name.
    Nested,
    /// A C preprocessor directive cpp prints nothing for and xrdb reads
    /// past (`#error`, `#pragma`, an unknown one): the key is its name.
    Directive,
}

/// Lines, 1-based, each without its CR.
fn lines(text: &[u8]) -> impl Iterator<Item = (Line, &[u8])> {
    text.split(|&b| b == b'\n')
        .zip(1u32..)
        .map(|(raw, n)| (Line(n), raw.strip_suffix(b"\r").unwrap_or(raw)))
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Where a palette number lands: 0-15 in [`Scheme::ansi`], 16-255 in
/// [`Scheme::extended`].
fn entry(s: &mut Scheme, n: u8) -> &mut Option<Color> {
    match n.checked_sub(16) {
        None => &mut s.ansi[usize::from(n)],
        Some(high) => &mut s.extended[usize::from(high)],
    }
}

/// What kitty's parser makes of `none` for a key: to_color_or_none's unset,
/// or to_color's refusal (`[[source: kitty-conf-utils-1e1486e]]` 54-62).
#[derive(Clone, Copy, PartialEq, Eq)]
enum NoneIs {
    Unset,
    Refused,
}

/// A key whose `none` changes what another key means (kitty.conf,
/// `[[source: kitty-options-definition]]` 353-379, 2641-2657).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Cursor,
    SelectionText,
    Other,
}

/// What the last line for a [`Role`] key said.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    Unsaid,
    Unset,
    Colour,
}

/// A decimal palette number, `0` to `255`.
fn decimal_index(t: &[u8]) -> Option<u8> {
    if t.is_empty() || t.len() > 3 || !t.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n = t.iter().fold(0u32, |n, &d| n * 10 + u32::from(d - b'0'));
    u8::try_from(n).ok()
}

/// What a kitty value is, by note AR's rules for kitty's colour values.
enum KittyValue {
    Colour(Color),
    /// `none`: unset, where the key takes it ([`NoneIs`]).
    Unset,
    /// A value kitty refuses, so kitty skips the line.
    Refused,
}

/// C's `isspace`: the white space a kitty value is trimmed of.
const C_SPACE: &[u8] = b" \t\n\x0b\x0c\r";

/// One component of `#` or `rgb:` (note AR's rules 4-6): one hex digit
/// times 17, else its first two characters as two hex digits, and nothing
/// after them read.
fn kitty_component(c: &[u8]) -> Option<u8> {
    match *c {
        [] => None,
        [d] => hex_digit(d).map(|v| v * 17),
        [h, l, ..] => Some(hex_digit(h)? * 16 + hex_digit(l)?),
    }
}

/// kitty's colour names (`[[source: kitty-color-names-1e1486e]]`), by the
/// table [`super::colour_names`] holds: FNV-1a picks a bucket, the
/// bucket's seed a slot, and the slot's name is compared, as kitty's gperf
/// table compares the whole name (in_color_name_set).
fn kitty_name(v: &[u8]) -> Option<[u8; 3]> {
    use super::colour_names::{POOL, SEEDS, SLOTS};
    crate::colour_name::find(&SEEDS, &SLOTS, POOL, v)
}

/// C's `strtod` in the C locale (note AR's rule 7): whether the whole
/// token is one number and `errno` stays 0. Leading `isspace` is skipped; then
/// a sign and `inf`, `infinity`, `nan`, `nan(...)`, a hexadecimal
/// `0x...p...` or a decimal. `ERANGE` is an overflow, or a result below
/// the smallest normal double that is not exact -- macOS's strtod_l,
/// run on each (`scripts/kitty-colour-oracle.sh`): a decimal there, and a
/// hexadecimal whose bits fall below 2^-1074.
fn c_double(t: &[u8]) -> bool {
    let t = &t[t.iter().take_while(|b| C_SPACE.contains(b)).count()..];
    let t = match t {
        [b'+' | b'-', rest @ ..] => rest,
        _ => t,
    };
    if t.eq_ignore_ascii_case(b"inf") || t.eq_ignore_ascii_case(b"infinity") {
        return true;
    }
    if t.len() >= 3 && t[..3].eq_ignore_ascii_case(b"nan") {
        return match &t[3..] {
            [] => true,
            [b'(', inner @ .., b')'] => inner
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'_'),
            _ => false,
        };
    }
    let digits = |t: &[u8], hex: bool| {
        t.iter()
            .take_while(|b| {
                if hex {
                    b.is_ascii_hexdigit()
                } else {
                    b.is_ascii_digit()
                }
            })
            .count()
    };
    let (hex, body) = match t {
        [b'0', b'x' | b'X', rest @ ..] => (true, rest),
        _ => (false, t),
    };
    let int = digits(body, hex);
    let (frac, after) = match body.get(int) {
        Some(b'.') => {
            let f = digits(&body[int + 1..], hex);
            (f, int + 1 + f)
        }
        _ => (0, int),
    };
    if int + frac == 0 {
        return false;
    }
    let mantissa = &body[..after];
    let exp = match &body[after..] {
        [] => Some(0i64),
        [m, rest @ ..] if m.eq_ignore_ascii_case(if hex { &b'p' } else { &b'e' }) => {
            let (neg, d) = match rest {
                [b'-', d @ ..] => (true, d),
                [b'+', d @ ..] => (false, d),
                _ => (false, rest),
            };
            if d.is_empty() || !d.iter().all(u8::is_ascii_digit) {
                return false;
            }
            let n = d.iter().fold(0i64, |n, &c| {
                n.saturating_mul(10).saturating_add(i64::from(c - b'0'))
            });
            Some(if neg { -n } else { n })
        }
        _ => None,
    };
    let Some(exp) = exp else { return false };
    if hex {
        return hex_in_range(mantissa, exp);
    }
    let Some(v) = core::str::from_utf8(t)
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
    else {
        return false;
    };
    let zero = mantissa.iter().all(|&b| b == b'0' || b == b'.');
    v.is_finite() && (zero || v.abs() >= f64::MIN_POSITIVE)
}

/// Whether a hexadecimal float, its digits and binary exponent, reads with
/// no `ERANGE`: not past `f64::MAX` once rounded to 53 bits, and exact if
/// it is below the smallest normal, 2^-1022.
fn hex_in_range(mantissa: &[u8], exp: i64) -> bool {
    let frac = mantissa
        .iter()
        .position(|&b| b == b'.')
        .map_or(0, |p| mantissa.len() - p - 1);
    let values = || {
        mantissa
            .iter()
            .filter(|&&b| b != b'.')
            .map(|&d| (d as char).to_digit(16).unwrap_or(0))
    };
    let n = i64::try_from(values().count()).unwrap_or(i64::MAX);
    let shift = exp.saturating_sub(4 * i64::try_from(frac).unwrap_or(i64::MAX));
    // Bit `k` of the digit `i` from the left is 2^(4 (n - 1 - i) + k + shift).
    let set = |pos: i64| {
        let q = pos.saturating_sub(shift);
        if q < 0 || q >= n.saturating_mul(4) {
            return false;
        }
        let i = usize::try_from(n - 1 - q / 4).unwrap_or(0);
        values().nth(i).is_some_and(|v| v >> (q % 4) & 1 == 1)
    };
    let (mut top, mut low) = (None::<i64>, None::<i64>);
    for (i, v) in (0..).zip(values()) {
        if v != 0 {
            let at = 4 * (n - 1 - i) + shift;
            top.get_or_insert(at.saturating_add(i64::from(31 - v.leading_zeros())));
            low = Some(at.saturating_add(i64::from(v.trailing_zeros())));
        }
    }
    let (Some(top), Some(low)) = (top, low) else {
        return true;
    };
    match top {
        1024.. => false,
        // Rounding carries to 2^1024 when the 53 bits are all set and the
        // bit below them is too (a tie rounds to even, and all ones is odd).
        1023 => !(971..=1023).all(set) || !set(970),
        -1022.. => true,
        _ => low >= -1074,
    }
}

/// The number C's `strtod` reads, for a token [`c_double`] accepts: a
/// decimal by Rust's parser, which rounds as `strtod` does; a hexadecimal
/// exactly, its significand rounded to 53 bits with every dropped digit
/// kept as a sticky bit.
fn c_double_value(t: &[u8]) -> f64 {
    let t = &t[t.iter().take_while(|b| C_SPACE.contains(b)).count()..];
    let (negative, body) = match t {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, t),
    };
    let v = if body.len() >= 3 && body[..3].eq_ignore_ascii_case(b"nan") {
        f64::NAN
    } else if body.eq_ignore_ascii_case(b"inf") || body.eq_ignore_ascii_case(b"infinity") {
        f64::INFINITY
    } else if let [b'0', b'x' | b'X', rest @ ..] = body {
        hex_value(rest)
    } else {
        core::str::from_utf8(body)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN)
    };
    if negative { -v } else { v }
}

/// A hexadecimal float's value after its `0x`, in range (as
/// [`hex_in_range`] has found it).
fn hex_value(t: &[u8]) -> f64 {
    let (mut m, mut shift, mut point) = (0u64, 0i64, false);
    let mut sticky = 0u64;
    let mut rest = t;
    while let [d, tail @ ..] = rest {
        rest = tail;
        match d {
            b'.' => point = true,
            b'p' | b'P' => break,
            _ => {
                let v = u64::from(hex_digit(*d).unwrap_or(0));
                if m < 1 << 60 {
                    m = m << 4 | v;
                    shift -= if point { 4 } else { 0 };
                } else {
                    sticky |= v;
                    shift += if point { 0 } else { 4 };
                }
            }
        }
    }
    let exp = match rest {
        [b'-', d @ ..] => -decimal_saturating(d),
        [b'+', d @ ..] => decimal_saturating(d),
        d => decimal_saturating(d),
    };
    #[allow(clippy::cast_precision_loss)]
    let mut v = (m | u64::from(sticky != 0)) as f64;
    let mut e = shift.saturating_add(exp);
    let two = |p: i64| f64::from_bits(u64::try_from(p + 1023).unwrap_or(0) << 52);
    while e > 1000 {
        v *= two(1000);
        e -= 1000;
    }
    while e < -1000 {
        v *= two(-1000);
        e += 1000;
    }
    v * two(e)
}

fn decimal_saturating(d: &[u8]) -> i64 {
    d.iter().fold(0i64, |n, &c| {
        n.saturating_mul(10).saturating_add(i64::from(c - b'0'))
    })
}

/// A number of rule 7, a trailing `%` dividing it by `percent`; `None`
/// where `strtod` does not read it whole.
fn c_intensity(t: &[u8], percent: f64) -> Option<f64> {
    match t.strip_suffix(b"%") {
        Some(n) => c_double(n).then(|| c_double_value(n) / percent),
        None => c_double(t).then(|| c_double_value(t)),
    }
}

/// A value's first three pieces between `seps`, empty pieces dropped.
fn three<'a>(v: &'a [u8], seps: &'a [u8]) -> Option<[&'a [u8]; 3]> {
    let mut pieces = v.split(|b| seps.contains(b)).filter(|p| !p.is_empty());
    Some([pieces.next()?, pieces.next()?, pieces.next()?])
}

/// `inner)` with `inner` under 256 bytes, its three numbers each divided
/// by `percent` where a `%` ends it (rule 7).
fn functional(v: &[u8], percent: f64) -> Option<[f64; 3]> {
    let inner = v.strip_suffix(b")").filter(|i| i.len() < 256)?;
    let [x, y, z] = three(inner, b" ,")?;
    Some([
        c_intensity(x, percent)?,
        c_intensity(y, percent)?,
        c_intensity(z, percent)?,
    ])
}

/// `v` without its `prefix`, the prefix's case ignored.
fn after<'a>(v: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    let (head, rest) = v.split_at_checked(prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then_some(rest)
}

/// A kitty colour value, by note AR's rules ("The kitty reader against
/// kitty's code, and its re-derivation"): trimmed of C's white space, case
/// ignored (1); its comment cut (2); then a name of kitty's table, or a
/// form (3): `#` and 3 digits, each doubled as CSS Color 4 doubles them
/// (4); `#` and 6, 9 or 12, each third's first two characters, X11's high
/// bits (5); `rgb:` (6); `oklch(` or `lab(` (7), drawn by
/// [`super::oklab`] as the owner's call on O14 has it. `rgbi:` and
/// anything else is refused (8), and its line is skipped, as kitty skips
/// it.
fn kitty_colour(raw: &[u8]) -> Result<KittyValue, SchemeErrorKind> {
    let v = trim_with(raw, C_SPACE);
    if v.eq_ignore_ascii_case(b"none") {
        return Ok(KittyValue::Unset);
    }
    let v = if v.first() == Some(&b'#') {
        v.split(|&b| b == b' ').next().unwrap_or(v)
    } else {
        trim_with(v.split(|&b| b == b'#').next().unwrap_or(v), C_SPACE)
    };
    let drawn = kitty_name(v).or_else(|| {
        if let Some(d) = v.strip_prefix(b"#") {
            let w = d.len() / 3;
            let [r, g, b] = match d.len() {
                3 => [&d[..1], &d[1..2], &d[2..]],
                6 | 9 | 12 => [&d[..2], &d[w..w + 2], &d[2 * w..2 * w + 2]],
                _ => return None,
            };
            Some([
                kitty_component(r)?,
                kitty_component(g)?,
                kitty_component(b)?,
            ])
        } else if let Some(d) = after(v, b"rgb:").filter(|d| d.len() < 32) {
            let [r, g, b] = three(d, b"/")?;
            Some([
                kitty_component(r)?,
                kitty_component(g)?,
                kitty_component(b)?,
            ])
        } else if let Some(d) = after(v, b"oklch(") {
            functional(d, 100.0).map(|[l, c, h]| super::oklab::oklch(l, c, h))
        } else {
            let d = after(v, b"lab(")?;
            functional(d, 1.0).map(|[l, a, b]| super::oklab::lab(l, a, b))
        }
    });
    Ok(drawn.map_or(KittyValue::Refused, |[r, g, b]| {
        KittyValue::Colour(rgb(r, g, b))
    }))
}

/// A kitty theme: kitty's own config (`[[source: kitty-conf]]`, "A theme in
/// kitty is just a .conf file containing kitty settings"). A line is
/// trimmed; a comment is a line whose first byte is `#`
/// (`[[source: kitty-conf]]` 1285-1287); a setting is `name value`, the name
/// `[a-zA-Z][a-zA-Z0-9_-]*` and at least one space before the value
/// (`[[source: kitty-conf-utils-1e1486e]]` 24, 287-293). Read: `foreground`, `background`,
/// `cursor`, `cursor_text_color`, `selection_foreground`,
/// `selection_background` and `color0`-`color255`
/// (`[[source: kitty-options-definition]]` 353, 369, 2470, 2641, 2655,
/// 2688), a later line winning. A value is read as kitty's parse_color
/// reads it, kitty's colour names included. Every other line goes to
/// `ignored`, and so does a value kitty itself refuses, as kitty skips that
/// line.
///
/// **kitty's special values are kept as the file says them**, from its
/// last line for each key (kitty.conf's text, the same lines), and resolved
/// where they are drawn, against the colours over them then: a config's
/// and a program's. `none` is a value of cursor, the selection's two and
/// color16 to color255, which leaves one unset, and is refused by every
/// other key, as kitty's to_color refuses it (`[[source:
/// kitty-conf-utils-1e1486e]]` 54-62). cursor none is "a 'reverse video'
/// effect", baud's swap, under which cursor_text_color "is ignored"
/// ([`CursorText::UnderACursorColour`]); cursor_text_color background,
/// "the background color of the cell underneath", is the cell's page
/// ([`CellPaint::Page`], `[[source: kitty-options-utils-1e1486e]]`
/// 637-640); selection_background none is the cell's text as the page,
/// kitty's renderer's reading too (`[[source:
/// kitty-1e1486e-background-slang]]` 302); selection_foreground none is
/// "the foreground color to be used unchanged"
/// ([`SelectionText::Unchanged`]): the cell's text over a selection page
/// of its own, and reverse video's text where the page in force is none,
/// as kitty's renderer has it (`[[source: kitty-1e1486e-shaders-c]]`
/// 873-885). Decided by Jev, M_FINAL, 1.00 both orders.
pub fn kitty(
    text: &[u8],
    mut ignored: impl FnMut(Line, &[u8], Skipped),
) -> Result<Scheme, SchemeError> {
    let mut s = Scheme::empty(Format::Kitty);
    let (mut cursor, mut selection_text) = (Last::Unsaid, Last::Unsaid);
    for (line, raw) in lines(text) {
        let b = trim_with(raw, b" \t\x0b\x0c");
        if b.is_empty() || b[0] == b'#' {
            continue;
        }
        let name_len = if b[0].is_ascii_alphabetic() {
            b.iter()
                .take_while(|&&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                .count()
        } else {
            0
        };
        let (name, rest) = b.split_at(name_len);
        let value = trim_with(rest, b" \t\x0b\x0c");
        if name.is_empty() || value.is_empty() || value.len() == rest.len() {
            ignored(line, b, Skipped::Syntax);
            continue;
        }
        let (slot, none, role) = match name {
            b"foreground" => (Target::Colour(&mut s.text), NoneIs::Refused, Role::Other),
            b"background" => (Target::Colour(&mut s.page), NoneIs::Refused, Role::Other),
            b"cursor" => (Target::Paint(&mut s.cursor), NoneIs::Unset, Role::Cursor),
            b"cursor_text_color" => (
                Target::Paint(&mut s.cursor_text),
                NoneIs::Refused,
                Role::Other,
            ),
            b"selection_foreground" => (
                Target::Paint(&mut s.selection_text),
                NoneIs::Unset,
                Role::SelectionText,
            ),
            b"selection_background" => {
                (Target::Paint(&mut s.selection), NoneIs::Unset, Role::Other)
            }
            _ => match name.strip_prefix(b"color") {
                // `color0` to `color255`, so `color01` is no setting.
                Some(d) if d.len() == 1 || d.first() != Some(&b'0') => match decimal_index(d) {
                    Some(n) => {
                        let none = match n < 16 {
                            true => NoneIs::Refused,
                            false => NoneIs::Unset,
                        };
                        (Target::Colour(entry(&mut s, n)), none, Role::Other)
                    }
                    None => {
                        ignored(line, name, Skipped::NotAColour);
                        continue;
                    }
                },
                _ => {
                    ignored(line, name, Skipped::NotAColour);
                    continue;
                }
            },
        };
        if name == b"cursor_text_color" && value.eq_ignore_ascii_case(b"background") {
            slot.set_cell(CellPaint::Page);
            continue;
        }
        let said = match kitty_colour(value).map_err(|kind| SchemeError { line, kind })? {
            KittyValue::Unset if none == NoneIs::Refused => KittyValue::Refused,
            v => v,
        };
        let last = match said {
            KittyValue::Colour(c) => {
                slot.set(Some(c));
                Last::Colour
            }
            KittyValue::Unset => {
                slot.set(None);
                Last::Unset
            }
            KittyValue::Refused => {
                ignored(line, name, Skipped::Refused);
                continue;
            }
        };
        match role {
            Role::Cursor => cursor = last,
            Role::SelectionText => selection_text = last,
            Role::Other => {}
        }
    }
    if cursor == Last::Unset {
        s.cursor_text_under = CursorText::UnderACursorColour;
    }
    if selection_text == Last::Unset {
        s.unset_selection_text = SelectionText::Unchanged;
    }
    Ok(s)
}

/// foot's `str_to_ulong(key, 0, ...)` for a palette index: C's `strtoul`
/// in base 0, so `0x` is hex and a leading `0` octal; below 256.
fn foot_index(k: &[u8]) -> Option<u8> {
    let (base, digits) = match k {
        [b'0', b'x' | b'X', rest @ ..] => (16, rest),
        [b'0', rest @ ..] if !rest.is_empty() => (8, rest),
        _ => (10, k),
    };
    if digits.is_empty() {
        return None;
    }
    let mut n = 0u32;
    for &d in digits {
        n = n * base + (d as char).to_digit(base)?;
        if n > 255 {
            return None;
        }
    }
    u8::try_from(n).ok()
}

/// foot's `value_to_color` without alpha (foot `config.c` 694-719): six
/// characters that C's `strtoul` reads whole in base 16 -- a `+` and a
/// `0x` allowed before the digits.
fn foot_colour(v: &[u8]) -> Option<Color> {
    if v.len() != 6 {
        return None;
    }
    let d = v.strip_prefix(b"+").unwrap_or(v);
    let d = match d {
        [b'0', b'x' | b'X', rest @ ..] => rest,
        _ => d,
    };
    if d.is_empty() {
        return None;
    }
    let mut n = 0u32;
    for &c in d {
        n = n * 16 + u32::from(hex_digit(c)?);
    }
    let [_, r, g, b] = n.to_be_bytes();
    Some(rgb(r, g, b))
}

/// Which colour section a foot line is in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FootSection {
    Dark,
    Light,
    Other,
}

/// A foot theme (`[[source: foot-2705e36-foot-ini-5]]`): INI, a line whose
/// first non-blank byte is `#` a comment, a blank then `#` starting a
/// trailing one, `[section]`, and `key=value` each side trimmed
/// (`[[source: foot-config]]` 3207-3304, 3027-3080). Read in
/// `[colors-dark]` and `[colors-light]`: `regular0`-`7`, `bright0`-`7`,
/// `0`-`255` (a C base-0 number), `foreground`, `background`,
/// `selection-foreground`, `selection-background`, and `cursor`, "Two space
/// separated RRGGBB values ... the foreground (text) and background
/// (cursor)" (`[[source: foot-2705e36-foot-ini-5]]` 1044-1051). Every
/// value is six hex digits, "without prefix" (1031). foot logs a line it
/// cannot read and goes on (`error_or_continue`), so a bad line here goes
/// to `ignored` and the rest is read.
///
/// **A file with both sections holds two themes**, foot's dark and light
/// one; which half is baud's to read is undecided (note AR, O8, below 0.9),
/// so the file is refused naming both, by the design's rule for two
/// readings that differ. Every other key and section goes to `ignored`.
pub fn foot(
    text: &[u8],
    mut ignored: impl FnMut(Line, &[u8], Skipped),
) -> Result<Scheme, SchemeError> {
    let mut s = Scheme::empty(Format::Foot);
    let mut section = FootSection::Other;
    let mut seen: [Option<Line>; 2] = [None, None];
    for (line, raw) in lines(text) {
        let mut b = trim_with(raw, b" \t\x0b\x0c");
        if b.is_empty() || b[0] == b'#' {
            continue;
        }
        if let Some(h) = b
            .windows(2)
            .position(|w| matches!(w[0], b' ' | b'\t') && w[1] == b'#')
        {
            b = trim_with(&b[..h], b" \t");
        }
        if let Some(name) = b.strip_prefix(b"[") {
            section = match name.strip_suffix(b"]") {
                Some(b"colors-dark") => FootSection::Dark,
                Some(b"colors-light") => FootSection::Light,
                _ => {
                    ignored(line, b, Skipped::Section);
                    FootSection::Other
                }
            };
            continue;
        }
        let half = match section {
            FootSection::Dark => 0,
            FootSection::Light => 1,
            // Reported once, at its header.
            FootSection::Other => continue,
        };
        let Some(eq) = b.iter().position(|&c| c == b'=') else {
            ignored(line, b, Skipped::Syntax);
            continue;
        };
        let key = trim_with(&b[..eq], b" \t");
        let value = trim_with(&b[eq + 1..], b" \t");
        if key.is_empty() || value.is_empty() {
            ignored(line, b, Skipped::Syntax);
            continue;
        }
        let slot = match key {
            b"foreground" => Target::Colour(&mut s.text),
            b"background" => Target::Colour(&mut s.page),
            b"selection-foreground" => Target::Paint(&mut s.selection_text),
            b"selection-background" => Target::Paint(&mut s.selection),
            b"cursor" => {
                // `strtok` on spaces: the first two values, a third ignored.
                let mut two = value.split(|&c| c == b' ').filter(|p| !p.is_empty());
                match (two.next().map(foot_colour), two.next().map(foot_colour)) {
                    (Some(Some(text_colour)), Some(Some(cursor))) => {
                        s.cursor_text = Some(CellPaint::Colour(text_colour));
                        s.cursor = Some(CellPaint::Colour(cursor));
                        seen[half].get_or_insert(line);
                    }
                    _ => ignored(line, key, Skipped::Refused),
                }
                continue;
            }
            [b'r', b'e', b'g', b'u', b'l', b'a', b'r', d @ b'0'..=b'7'] => {
                Target::Colour(&mut s.ansi[usize::from(d - b'0')])
            }
            [b'b', b'r', b'i', b'g', b'h', b't', d @ b'0'..=b'7'] => {
                Target::Colour(&mut s.ansi[usize::from(d - b'0' + 8)])
            }
            [d, ..] if d.is_ascii_digit() => match foot_index(key) {
                Some(n) => Target::Colour(entry(&mut s, n)),
                None => {
                    ignored(line, key, Skipped::Refused);
                    continue;
                }
            },
            _ => {
                ignored(line, key, Skipped::NotAColour);
                continue;
            }
        };
        match foot_colour(value) {
            Some(c) => {
                slot.set(Some(c));
                seen[half].get_or_insert(line);
            }
            None => ignored(line, key, Skipped::Refused),
        }
    }
    if let [Some(_), Some(light)] = seen {
        return Err(SchemeError {
            line: light,
            kind: SchemeErrorKind::Pair,
        });
    }
    Ok(s)
}

/// A list of sixteen colours, `#rrggbb` a line, ANSI 0 to 15 in order, as
/// pywal's `colors` file holds them: `{color0}` to `{color15}`, one a line
/// (`[[source: pywal-colors-template-236aa48]]`), each written `#%02x%02x%02x`
/// (`[[source: pywal-util-236aa48]]` 22-23, 173-175); the owner's answer to
/// note AR's O9, 2026-10-17.
/// No page or text is named, so both stay baud's own (Jev, O11). The
/// detector has already found the sixteen lines and nothing else.
pub fn plain_list(text: &[u8]) -> Result<Scheme, SchemeError> {
    let mut s = Scheme::empty(Format::PlainList);
    let mut n = 0usize;
    for (line, raw) in lines(text) {
        let b = trim_with(raw, b" \t");
        if b.is_empty() {
            continue;
        }
        let c = super::hex(b).map_err(|e| SchemeError {
            line,
            kind: SchemeErrorKind::Hex(e),
        })?;
        let slot = s.ansi.get_mut(n).ok_or(SchemeError {
            line,
            kind: SchemeErrorKind::Count,
        })?;
        *slot = Some(c);
        n += 1;
    }
    if n != 16 {
        return Err(SchemeError {
            line: Line(u32::try_from(n).unwrap_or(u32::MAX)),
            kind: SchemeErrorKind::Count,
        });
    }
    Ok(s)
}
