//! **The read record checked against the core**, shared by the generated
//! programs (`read_record_props.rs`) and the corpus drawn from the core's
//! own parse table (`read_record_corpus.rs`): a terminal whose every line
//! of both screens is marked, both screens' lines as the record numbers
//! them, and what a read's record leaves unsaid of what the read did.
#![allow(dead_code)]
use baud_core::*;
use baud_domain::{
    BytePos, CacheGeometry, Depth, LineIdx, PackedCell, ReadLen, Row, Rows, WinSize,
};
use std::collections::BTreeMap;

pub const WS: WinSize = WinSize {
    ws_col: 12,
    ws_row: 8,
};

/// A terminal of [`WS`] with a short history, so a read can scroll a line
/// into it and the snapshot stays small, every line of both screens marked
/// ([`marked_lines`]) and its record taken.
pub fn with<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(6));
    let mut t = Term::new(owned.storage(), &WS);
    let marked = marked_lines();
    t.write_at(BytePos::ZERO, &marked);
    t.feed(BytePos::ZERO, ReadLen::from_index(marked.len()));
    let _ = t.take_record();
    f(&mut t)
}

/// Every line of `screen` the terminal holds, up or parked, by the
/// record's numbering: its screen rows, and on the primary its history's.
pub fn lines(t: &Term<'_>, screen: Screen) -> BTreeMap<LineIdx, Vec<PackedCell>> {
    let (cols, rows) = t.geometry();
    let (v, top, hist) = t.record_surface(screen);
    let mut out = BTreeMap::new();
    for y in 0..rows.index() {
        let cells = v.row(Row::from_index(y))[..cols.index()].to_vec();
        out.insert(top.plus(Rows::of_count(y)), cells);
    }
    for back in 1..=hist.index() {
        if let Some(line) = top.minus(Rows::of_count(back)) {
            out.insert(line, v.back(back).row(Row::ZERO)[..cols.index()].to_vec());
        }
    }
    out
}

/// Both surfaces, the primary first.
pub const SURFACES: [Screen; 2] = [Screen::Normal, Screen::Alternate];

/// Both surfaces' lines.
pub fn both(t: &Term<'_>) -> [BTreeMap<LineIdx, Vec<PackedCell>>; 2] {
    SURFACES.map(|s| lines(t, s))
}

/// **The markers**: a codepoint nothing a test feeds can write, one per
/// line, the primary's from `U+10300` (Old Italic) and the alternate
/// screen's from `U+10330` (Gothic), so a marker found on a line says
/// which line its text came from. In the astral plane because the
/// character sets reach the BMP: a single shift into a 96-character set
/// writes `U+0152` from a `W`, which a Latin marker read as moved text. And
/// past what the table walk writes, whose four-byte characters are
/// `F0 90 80 xx`, the first a cut lead admits.
pub const PRIMARY_MARK: u32 = 0x10300;
pub const ALTERNATE_MARK: u32 = 0x10330;
pub const MARKS: core::ops::Range<u32> = PRIMARY_MARK..ALTERNATE_MARK + 0x20;

/// The marker a line was filled with, by the lines the terminal starts on.
pub fn mark(base: u32, i: usize) -> String {
    let c = char::from_u32(base + u32::try_from(i).expect("a few lines")).expect("a scalar");
    format!("{c}.....")
}

/// **Every line marked**: the primary's screen and history, then the
/// alternate screen's rows under `?47`, which keeps them; the last
/// character printed a full stop, so a repeat (`REP`) repeats no marker.
pub fn marked_lines() -> Vec<u8> {
    let rows = usize::from(WS.ws_row);
    let lines = rows + 6;
    let mut s = String::new();
    for i in 0..lines {
        s.push_str(&mark(PRIMARY_MARK, i));
        if i + 1 < lines {
            s.push_str("\r\n");
        }
    }
    s.push_str("\x1b[?47h");
    for j in 0..rows {
        s.push_str(&format!("\x1b[{};1H{}", j + 1, mark(ALTERNATE_MARK, j)));
    }
    s.push_str("\x1b[?47l\x1b[1;1H");
    s.into_bytes()
}

/// Where each marker stands: the marker, its surface and its line.
pub fn markers(snap: &[BTreeMap<LineIdx, Vec<PackedCell>>; 2]) -> Vec<(u32, Screen, LineIdx)> {
    let mut out = Vec::new();
    for (s, lines) in SURFACES.iter().zip(snap) {
        for (line, cells) in lines {
            for c in cells {
                if let Some(v) = c.content.as_codepoint().map(|c| c.value()) {
                    if MARKS.contains(&v) && !out.contains(&(v, *s, *line)) {
                        out.push((v, *s, *line));
                    }
                }
            }
        }
    }
    out
}

/// What one read did that its record does not say.
#[derive(Debug, Default)]
pub struct Unsaid {
    /// Lines whose cells changed that the record names nowhere.
    pub unnamed: Vec<(Screen, LineIdx)>,
    /// Markers found on a line their text was not on before the read, with
    /// no move of the record's to bring it there: (marker, surface, line).
    pub displaced: Vec<(u32, Screen, LineIdx)>,
}

/// **What `rec` leaves unsaid** of a read that took the terminal from
/// `before` to `after`. A record that renumbered says nothing local, and
/// the alternate screen, where the read cleared it, is numbered again.
pub fn unsaid(
    rec: &ReadRecord,
    before: &[BTreeMap<LineIdx, Vec<PackedCell>>; 2],
    after: &[BTreeMap<LineIdx, Vec<PackedCell>>; 2],
) -> Unsaid {
    let mut out = Unsaid::default();
    if rec.renumbered().is_some() {
        return out;
    }
    let read = |s: Screen| !(s == Screen::Alternate && rec.cleared() == Cleared::Alternate);
    for (k, s) in SURFACES.iter().enumerate().filter(|(_, s)| read(**s)) {
        out.unnamed.extend(
            before[k]
                .iter()
                .filter(|(l, cells)| after[k].get(l).is_some_and(|now| now != *cells))
                .map(|(l, _)| (*s, *l))
                .filter(|(s, l)| !named(rec, *s, *l)),
        );
    }
    let was = markers(before);
    for (m, s, line) in markers(after) {
        let from: Vec<LineIdx> = was
            .iter()
            .filter(|&&(w, ws, _)| w == m && ws == s)
            .map(|&(_, _, l)| l)
            .collect();
        let sources = rec.carried_from(s, line);
        let explained = !read(s) || from.iter().any(|l| sources.holds(*l));
        if !explained {
            out.displaced.push((m, s, line));
        }
    }
    out
}

/// **Whether `rec` names `line` of `screen`** as one the read may have
/// written: on its forward path there, off it, or where a move took text.
pub fn named(rec: &ReadRecord, screen: Screen, line: LineIdx) -> bool {
    let ends = [rec.start(), rec.end()]
        .into_iter()
        .filter(|a| a.screen == screen)
        .map(|a| Span::of(a.line, a.line))
        .chain(rec.off_path(screen))
        .reduce(Span::hull);
    let moved = rec.moves().iter().any(|m| match m {
        Move::Exact {
            screen: s,
            from,
            by,
            ..
        } => s == screen && from.moved(by).is_some_and(|to| to.holds(line)),
        Move::Within { screen: s, band } => s == screen && band.holds(line),
    });
    ends.is_some_and(|s| s.holds(line)) || moved
}

/// **One read fed whole**, its record taken, and what the record leaves
/// unsaid of what the read did to both screens.
pub fn checked(t: &mut Term<'_>, bytes: &[u8]) -> (ReadRecord, Unsaid) {
    let before = both(t);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let rec = t.take_record();
    let unsaid = unsaid(&rec, &before, &both(t));
    (rec, unsaid)
}
