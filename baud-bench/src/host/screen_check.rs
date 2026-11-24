//! **What a terminal in the pty comparison says about its screen after a
//! replay, against what baud-core says.** `scripts/vt-child.sh`, once the
//! recording's bytes are written and before it resets the terminal (a reset
//! erases the screen), sends [`questions`]: the cursor position (DSR 6), a
//! checksum of the whole screen (DECRQCRA), and the cursor put at [`MARK`]
//! and asked for again so the reader knows the replies have all arrived. It
//! keeps what came back as hex. [`read`] takes that reply apart, and
//! [`reference`] puts the same questions to a headless baud-core fed the same
//! bytes at the same size.
//!
//! * The **cursor** is asked of every terminal, since every terminal answers
//!   DSR 6. It is one cell: a terminal that agrees on it and nothing more is
//!   [`Verdict::CursorOnly`], never the same screen.
//! * The **checksum** is compared for baud alone, whose DECRQCRA sums the
//!   reference's way. It is a sum over the cells, so it sees a cell whose
//!   character or rendition differs and not text moved along its row with
//!   nothing else changed: [`Verdict::Same`] says the two answers agree,
//!   not that every cell was compared. Where another terminal answers DECRQCRA it sums by a
//!   rule of its own, so its checksum is kept in the results and never
//!   compared.
//!
//! The reference has no image storage, so it is a check for text
//! recordings: a picture placed by the terminal and refused here would move
//! the cursor on one side only.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ReadLen, WinSize};

/// The id the checksum request carries, so its report is told apart from
/// any other device control string in the reply.
pub const CHECKSUM_ID: u16 = 4242;

/// Where the cursor is put before the last DSR 6: the reply the reader waits
/// for, `vt-reply 10 13 37`.
pub const MARK: (u16, u16) = (13, 37);

/// The bytes sent after the replay: the synchronised update ended, the
/// cursor asked for, the checksum of the page from (1, 1) to (`rows`,
/// `cols`) asked for, then the cursor put at [`MARK`] and asked for again.
pub fn questions(rows: u16, cols: u16) -> Vec<u8> {
    format!(
        "\x1b[?2026l\x1b[6n\x1b[{CHECKSUM_ID};1;1;1;{rows};{cols}*y\x1b[{};{}H\x1b[6n",
        MARK.0, MARK.1
    )
    .into_bytes()
}

/// What one reply says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Said {
    /// The cursor after the replay, (row, column) from one.
    pub cursor: Option<(u16, u16)>,
    /// The checksum report's four hex digits, as a number.
    pub checksum: Option<u16>,
}

/// The cursor reports in `reply`, `ESC [ row ; col R` or `CSI row ; col R`,
/// in order. A report with a `?` (DECXCPR) is not one.
fn cursor_reports(reply: &[u8]) -> Vec<(u16, u16)> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < reply.len() {
        let body = match reply[i] {
            0x1b if reply.get(i + 1) == Some(&b'[') => i + 2,
            0x9b => i + 1,
            _ => {
                i += 1;
                continue;
            }
        };
        let mut j = body;
        let mut nums = [0u32; 2];
        let mut which = 0;
        let mut digits = 0;
        while let Some(&b) = reply.get(j) {
            match b {
                b'0'..=b'9' if digits < 6 => {
                    nums[which] = nums[which] * 10 + u32::from(b - b'0');
                    digits += 1;
                }
                b';' if which == 0 && digits > 0 => {
                    which = 1;
                    digits = 0;
                }
                _ => break,
            }
            j += 1;
        }
        if which == 1 && digits > 0 && reply.get(j) == Some(&b'R') {
            if let (Ok(r), Ok(c)) = (u16::try_from(nums[0]), u16::try_from(nums[1])) {
                found.push((r, c));
            }
            i = j + 1;
        } else {
            i = body;
        }
    }
    found
}

/// The checksum in the report carrying [`CHECKSUM_ID`]: `DCS id ! ~ hhhh
/// ST`, the string introduced by `ESC P` or 0x90.
fn checksum_report(reply: &[u8]) -> Option<u16> {
    let id = CHECKSUM_ID.to_string();
    let mut tail = Vec::with_capacity(id.len() + 2);
    tail.extend_from_slice(id.as_bytes());
    tail.extend_from_slice(b"!~");
    (0..reply.len()).find_map(|i| {
        let start = match reply[i] {
            0x1b if reply.get(i + 1) == Some(&b'P') => i + 2,
            0x90 => i + 1,
            _ => return None,
        };
        let rest = reply.get(start..)?;
        let hex = rest.strip_prefix(tail.as_slice())?.get(..4)?;
        u16::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()
    })
}

/// Reads a reply. Of its cursor reports the last answers the question at
/// [`MARK`] and the one before it is the cursor after the replay: a
/// terminal answers in order, so the replies to the recording's own queries
/// come before both. With one report or none the cursor is not known.
pub fn read(reply: &[u8]) -> Said {
    let reports = cursor_reports(reply);
    let cursor = match reports.as_slice() {
        [.., before, last] if *last == MARK => Some(*before),
        _ => None,
    };
    Said {
        cursor,
        checksum: checksum_report(reply),
    }
}

/// The bytes `vt-reply` printed as hex, or `None` for `none` or anything
/// not hex.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let t = text.trim();
    if !t.len().is_multiple_of(2) || t.is_empty() {
        return None;
    }
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(t.get(i..i + 2)?, 16).ok())
        .collect()
}

/// `bytes` as a terminal reads them after a pty's default output processing
/// (`OPOST` and `ONLCR`, on for a fresh pty and in the comparison's `cat`):
/// every line feed preceded by a carriage return.
pub fn through_the_tty(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 32);
    for &b in bytes {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

/// One read into `t`: written to the arena, then parsed.
fn feed(t: &mut Term<'_>, chunk: &[u8]) {
    t.write_at(BytePos::ZERO, chunk);
    t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
}

/// A headless baud-core of `rows` x `cols`, fed `stream` in reads of 4096
/// bytes, then `plant` (empty for none), then [`questions`]; every reply it
/// owed on the way, in order.
pub fn replies(stream: &[u8], plant: &[u8], rows: u16, cols: u16) -> Vec<u8> {
    let ws = WinSize {
        ws_row: rows,
        ws_col: cols,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let asked = questions(rows, cols);
    let mut reply = Vec::new();
    for chunk in stream
        .chunks(4096)
        .chain(plant.chunks(4096))
        .chain(std::iter::once(asked.as_slice()))
    {
        feed(&mut t, chunk);
        reply.extend_from_slice(t.reply());
        t.clear_reply();
    }
    reply
}

/// What baud-core says after `stream` at `rows` x `cols`: the reference.
pub fn reference(stream: &[u8], rows: u16, cols: u16) -> Said {
    read(&replies(stream, &[], rows, cols))
}

/// How a terminal's answer differs from the reference's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Differs {
    /// The cursor after the replay: the terminal's, then baud-core's.
    Cursor((u16, u16), (u16, u16)),
    /// baud's checksum of the screen, then baud-core's.
    Checksum(u16, u16),
}

/// What one terminal's answer is worth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// baud: the cursor and the screen's checksum both equal the
    /// reference's. The checksum is a sum, so this is agreement of the two
    /// answers, not a cell-by-cell comparison.
    Same,
    /// The cursor equals the reference's; the screen was not compared.
    CursorOnly,
    /// The answer differs from the reference's.
    Unequal(Differs),
    /// The reference gives no cursor to compare: its cursor after the
    /// replay is [`MARK`] itself, where the reader stops, or the screen is
    /// smaller than [`MARK`], which the cursor cannot then reach.
    AtMark,
    /// No cursor came back to compare.
    NoReply,
}

impl Verdict {
    /// Whether a rate for this terminal on this recording may be quoted:
    /// only where nothing differs and something was compared.
    pub fn rated(self) -> bool {
        matches!(self, Verdict::Same | Verdict::CursorOnly)
    }

    /// The words printed for it.
    pub fn says(self) -> String {
        match self {
            Verdict::Same => "SAME (cursor and checksum agree with baud-core)".to_string(),
            Verdict::CursorOnly => {
                "CURSOR ONLY (the cursor agrees; the screen not compared)".to_string()
            }
            Verdict::Unequal(Differs::Cursor((r, c), (wr, wc))) => {
                format!("UNEQUAL (cursor {r};{c}, baud-core {wr};{wc}), no rate")
            }
            Verdict::Unequal(Differs::Checksum(s, w)) => {
                format!("UNEQUAL (checksum {s:04X}, baud-core {w:04X}), no rate")
            }
            Verdict::AtMark => {
                "NOT COMPARED (baud-core's cursor cannot be told from the mark), no rate"
                    .to_string()
            }
            Verdict::NoReply => "NO REPLY (no cursor came back), no rate".to_string(),
        }
    }
}

/// The verdict on `terminal`'s answer `said` against the reference `want`.
/// The checksum is compared for `baud` alone.
pub fn judge(terminal: &str, said: Said, want: Said) -> Verdict {
    let Some(want_at) = want.cursor.filter(|at| *at != MARK) else {
        return Verdict::AtMark;
    };
    let Some(at) = said.cursor else {
        return Verdict::NoReply;
    };
    if at != want_at {
        return Verdict::Unequal(Differs::Cursor(at, want_at));
    }
    if terminal != "baud" {
        return Verdict::CursorOnly;
    }
    match (said.checksum, want.checksum) {
        (Some(s), Some(w)) if s != w => Verdict::Unequal(Differs::Checksum(s, w)),
        (Some(_), Some(_)) => Verdict::Same,
        _ => Verdict::CursorOnly,
    }
}

/// The verdict on one line of `vt-compare.sh`'s results: `name recording
/// rowsxcols bytes t0 t1 cpu comm reply screen`, the recording found by its
/// file name among `recordings` and replayed `bytes / its length` times. A
/// line from before the screen was asked has no tenth field. The stream is
/// what the terminal read: `cat` wrote it through the tty's output
/// processing ([`through_the_tty`]).
pub fn judge_line(
    line: &str,
    recordings: &[(String, Vec<u8>)],
) -> Result<(String, String, Verdict), String> {
    let f: Vec<&str> = line.split_whitespace().collect();
    let [name, rec, size, total, ..] = f.as_slice() else {
        return Err(format!("too few fields: {line:?}"));
    };
    let screen = f
        .get(9)
        .ok_or_else(|| format!("{name} {rec}: the screen was not asked"))?;
    let (rows, cols) = size
        .split_once('x')
        .and_then(|(r, c)| Some((r.parse::<u16>().ok()?, c.parse::<u16>().ok()?)))
        .ok_or_else(|| format!("{name} {rec}: no size in {size:?}"))?;
    let bytes = recordings
        .iter()
        .find(|(file, _)| file == rec)
        .map(|(_, b)| b)
        .ok_or_else(|| format!("{name} {rec}: recording not given"))?;
    let total: usize = total
        .parse()
        .map_err(|_| format!("{name} {rec}: no byte count in {total:?}"))?;
    if bytes.is_empty() || !total.is_multiple_of(bytes.len()) {
        return Err(format!("{name} {rec}: {total} bytes is not whole replays"));
    }
    let stream = through_the_tty(&bytes.repeat(total / bytes.len()));
    let said = unhex(screen).map_or(
        Said {
            cursor: None,
            checksum: None,
        },
        |r| read(&r),
    );
    let want = reference(&stream, rows, cols);
    Ok((
        (*name).to_string(),
        (*rec).to_string(),
        judge(name, said, want),
    ))
}
