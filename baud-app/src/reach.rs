//! **Whether a read can have written on the line its cursor ended on**,
//! judged on the thread taking the reads as the bytes go by, with the
//! scan's state carried from one read to the next, so a sequence split
//! between two reads is judged by its final byte.
//!
//! A read's rows are filed under whoever wrote them (`paths::LineSpan`).
//! A command's output usually ends with a line feed, which leaves the
//! cursor on a line the read wrote nothing on, where the shell draws its
//! next prompt: that line is the prompt's, and filing it under the command
//! would darken the prompt after a remote or unnamed one. But a line feed
//! as the last byte does not show the line is clean: text on the line,
//! `CR`, `CSI A`, `LF` ends at its start with the text still on it. So the
//! line is left out only when nothing in the read could have moved the
//! cursor up or across a line, or written but at the cursor as it went:
//! printable text, `CR`, `LF`, `VT`, `FF` and `HT`, `SGR` (`CSI ... m`, no
//! private marker, no intermediate) and control strings (`OSC`, `DCS`,
//! `APC`, `PM`, `SOS`). Anything else -- any other `CSI` or `ESC`
//! sequence, any other control -- keeps the line in, which fails closed.
//! `BS` keeps it in too: under `?45` or `?1045`, which baud implements, a
//! `BS` at the first column moves to the line above. Under `ESC % @` a
//! byte from 0x80 to 0x9F is an 8-bit control, `CSI` among them, so after
//! one, until `ESC % G`, such a byte keeps the line in as well.
//!
//! **A line feed holds through what writes no cell**: a colour, a `CR` or
//! a tab, and an `OSC` -- a title, a folder report, a mark, a link opened
//! or closed, an iTerm badge -- leave the line the feed reached clean, so
//! a read ending `logout\r\n` and a badge reset, or a command's echo and
//! the shell's `C`, leaves it out. An `OSC 1337` handing over an inline
//! picture (`File=`, `MultipartFile=`, `FilePart=`) draws cells, and so
//! may a `DCS` (sixel, ReGIS) or an `APC` (kitty's pictures): after any of
//! those the line is kept in, and after a `PM` or `SOS`, which baud reads
//! nothing from, too.

use baud_core::StreamByte;

/// **What a read did to the line its cursor ended on**, or a drain of
/// reads ([`Reach::then`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Reach {
    /// Something in it could have moved the cursor up or across a line,
    /// or the scan cannot tell: it may have written on any line, the one
    /// it ended on included.
    #[default]
    Anywhere,
    /// Text and forward motion only, not ending on a line feed: the line
    /// it ended on may hold what it wrote.
    Forward,
    /// Text and forward motion only, ending on a line feed: the line it
    /// ended on holds nothing it wrote.
    Below,
}

impl Reach {
    /// **This read's reach and then `next`'s**, as one drain: below only
    /// when neither could move the cursor up and the last ends on a line
    /// feed, since an earlier read that moved up may have written on the
    /// line the last one ends on.
    pub fn then(self, next: Reach) -> Reach {
        match (self, next) {
            (Reach::Anywhere, _) | (_, Reach::Anywhere) => Reach::Anywhere,
            (_, n) => n,
        }
    }
}

/// Where the scan is in the stream.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum At {
    #[default]
    Ground,
    Esc,
    /// After `ESC %`: the byte that selects an encoding.
    Percent,
    /// After `ESC` and an intermediate, to the final byte, as the core's
    /// `escape intermediate` row reads it: a control there is carried out
    /// and `DEL` ignored, and the sequence goes on.
    EscInter,
    Csi(Params),
    /// Inside a control string, of this kind.
    Str(Kind),
    /// After an `ESC` inside a control string.
    StrEsc(Kind),
}

/// **What a control string can do to the screen**: an `OSC` as far as its
/// body so far tells, or any other, which may draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Osc(Body),
    Other,
}

/// **How far an `OSC` body spells an inline picture's**: the bytes still to
/// match and what matching them makes it, or settled either way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Body {
    Rest(&'static [u8], Then),
    /// Past `1337;`: `F` or `M` decides.
    Fork,
    Draws,
    Writes,
}

/// What a body that matched its bytes so far becomes once all match.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Then {
    Fork,
    Draws,
}

impl Body {
    const START: Body = Body::Rest(b"1337;", Then::Fork);

    /// The body after `b`.
    fn after(self, s: StreamByte) -> Body {
        let b = s.raw_byte();
        match self {
            Body::Rest(want, then) => match want.split_first() {
                Some((&w, rest)) if w == b => match (rest.is_empty(), then) {
                    (false, _) => Body::Rest(rest, then),
                    (true, Then::Fork) => Body::Fork,
                    (true, Then::Draws) => Body::Draws,
                },
                _ => Body::Writes,
            },
            Body::Fork => match b {
                b'F' => Body::Rest(b"ile", Then::Draws),
                b'M' => Body::Rest(b"ultipartFile", Then::Draws),
                _ => Body::Writes,
            },
            Body::Draws | Body::Writes => self,
        }
    }
}

impl Kind {
    /// Whether the string, ended, may have drawn a cell.
    fn draws(self) -> bool {
        !matches!(self, Kind::Osc(Body::Rest(..) | Body::Fork | Body::Writes))
    }
}

/// Whether a `CSI` so far could still be an `SGR`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Params {
    Plain,
    Other,
}

/// What a byte from 0x80 to 0x9F is: part of a UTF-8 character, or under
/// `ESC % @` an 8-bit control.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Encoding {
    #[default]
    Utf8,
    Latin1,
}

/// Whether anything in the read so far could have moved the cursor up or
/// across a line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Moved {
    No,
    Maybe,
}

/// Whether the read's last byte was a line feed in text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Last {
    LineFeed,
    Other,
}

/// **Whether any byte of the last read was scanned under `ESC % @`**,
/// where 0x80-0x9F are controls and an `OSC` may begin with 0x9D.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Latin1Seen {
    #[default]
    No,
    Yes,
}

/// **The scan over the reads**: one per reader, carried across reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reads {
    at: At,
    encoding: Encoding,
    latin1: Latin1Seen,
}

impl Reads {
    /// **What the read of `bytes` did to the line its cursor ended on.**
    pub fn read(&mut self, bytes: &[u8]) -> Reach {
        let mut moved = Moved::No;
        let mut last = Last::Other;
        self.latin1 = match self.encoding {
            Encoding::Latin1 => Latin1Seen::Yes,
            Encoding::Utf8 => Latin1Seen::No,
        };
        // The byte after an `ESC`, as the core's `escape` row reads it: a
        // control is carried out there and `DEL` ignored, and the sequence
        // goes on; an intermediate begins `escape intermediate`.
        let after_esc = |b, moved: &mut Moved| match b {
            0x18 | 0x1a => {
                *moved = Moved::Maybe;
                At::Ground
            }
            0x00..=0x1f | 0x7f => {
                *moved = Moved::Maybe;
                At::Esc
            }
            b'[' => At::Csi(Params::Plain),
            b']' => At::Str(Kind::Osc(Body::START)),
            b'P' | b'_' | b'^' | b'X' => At::Str(Kind::Other),
            b'%' => At::Percent,
            0x20..=0x2f => {
                *moved = Moved::Maybe;
                At::EscInter
            }
            _ => {
                *moved = Moved::Maybe;
                At::Ground
            }
        };
        for &b in bytes {
            match self.at {
                At::Ground => match b {
                    0x1b => self.at = At::Esc,
                    b'\n' | 0x0b | 0x0c => last = Last::LineFeed,
                    // Neither writes a cell.
                    b'\r' | b'\t' => {}
                    0x20..=0x7e | 0xa0..=0xff => last = Last::Other,
                    0x80..=0x9f if self.encoding == Encoding::Utf8 => last = Last::Other,
                    _ => moved = Moved::Maybe,
                },
                At::Esc => self.at = after_esc(b, &mut moved),
                // Only a direct `@` or `G` is read as the core reads it; after
                // anything else the scan takes 0x80-0x9F as controls until
                // `ESC % G`, which fails closed.
                At::Percent => {
                    moved = Moved::Maybe;
                    (self.at, self.encoding) = match b {
                        b'@' => (At::Ground, Encoding::Latin1),
                        b'G' => (At::Ground, Encoding::Utf8),
                        0x1b => (At::Esc, Encoding::Latin1),
                        0x00..=0x2f | 0x7f => (At::EscInter, Encoding::Latin1),
                        _ => (At::Ground, Encoding::Latin1),
                    };
                    if self.encoding == Encoding::Latin1 {
                        self.latin1 = Latin1Seen::Yes;
                    }
                }
                At::EscInter => {
                    moved = Moved::Maybe;
                    self.at = match b {
                        0x1b => At::Esc,
                        0x18 | 0x1a | 0x30..=0x7e => At::Ground,
                        // Under UTF-8 a byte from 0x80 ends the sequence and
                        // is the ground's.
                        0x80..=0xff if self.encoding == Encoding::Utf8 => {
                            last = Last::Other;
                            At::Ground
                        }
                        _ => At::EscInter,
                    };
                }
                At::Csi(p) => match b {
                    b'0'..=b'9' | b':' | b';' => {}
                    0x20..=0x2f | 0x3c..=0x3f => self.at = At::Csi(Params::Other),
                    b'm' if p == Params::Plain => self.at = At::Ground,
                    0x1b => {
                        moved = Moved::Maybe;
                        self.at = At::Esc;
                    }
                    // A final other than SGR's, or CAN and SUB, which
                    // abandon the sequence; any other control inside it is
                    // carried out there.
                    0x40..=0x7e | 0x18 | 0x1a => {
                        moved = Moved::Maybe;
                        self.at = At::Ground;
                    }
                    _ => moved = Moved::Maybe,
                },
                At::Str(kind) => match b {
                    0x07 | 0x18 | 0x1a => {
                        if kind.draws() {
                            last = Last::Other;
                        }
                        self.at = At::Ground;
                    }
                    0x1b => self.at = At::StrEsc(kind),
                    0x9c if self.encoding == Encoding::Latin1 => {
                        if kind.draws() {
                            last = Last::Other;
                        }
                        self.at = At::Ground;
                    }
                    _ => {
                        self.at = At::Str(match kind {
                            Kind::Osc(body) => Kind::Osc(body.after(StreamByte::from_stream(b))),
                            Kind::Other => Kind::Other,
                        });
                    }
                },
                At::StrEsc(kind) => match b {
                    b'\\' => {
                        if kind.draws() {
                            last = Last::Other;
                        }
                        self.at = At::Ground;
                    }
                    // An `ESC` that is not ST abandons the string and
                    // begins a sequence of its own.
                    _ => {
                        moved = Moved::Maybe;
                        self.at = after_esc(b, &mut moved);
                    }
                },
            }
        }
        match (moved, self.at, last) {
            (Moved::Maybe, _, _) => Reach::Anywhere,
            (Moved::No, At::Ground, Last::LineFeed) => Reach::Below,
            (Moved::No, _, _) => Reach::Forward,
        }
    }

    /// **What a byte from 0x80 to 0x9F is now**: part of a character, or
    /// under `ESC % @` an 8-bit control, `OSC` among them.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Whether any byte of the last read was scanned under `ESC % @`.
    pub fn latin1_seen(&self) -> Latin1Seen {
        self.latin1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reach(bytes: &[u8]) -> Reach {
        Reads::default().read(bytes)
    }

    /// **A read leaves its cursor's line out only when nothing in it could
    /// have moved the cursor up**: text, colour and control strings ending
    /// on a line feed leave it; a line feed after `CSI A`, an erase, a
    /// save and restore, `ESC M`, a private mode, a `BS`, or a bare `ESC`
    /// keep it, and so does a read not ending on a line feed. Named break:
    /// read the last byte alone -- the `CSI A` read is left out.
    #[test]
    fn only_text_and_forward_motion_leave_the_line_out() {
        for clean in [
            &b"hi\r\n"[..],
            b"\x1b[01;34mdir\x1b[0m  file\r\n",
            b"a\tb\x1b]8;;file:///x\x1b\\x\x1b]8;;\x07\r\n",
            b"\xc3\xa9t\xc3\xa9 \xe2\x82\xac\n",
            b"\x1bP1$r0m\x1b\\done\x0b",
        ] {
            assert_eq!(reach(clean), Reach::Below, "{clean:?}");
        }
        for moves in [
            &b"text\r\x1b[A\n"[..],
            b"text\x1b[K\r\n",
            b"\x1b7text\x1b8\n",
            b"text\x1bM\n",
            b"\x1b[?2004htext\n",
            b"\x1b[>4mtext\n",
            b"te\x08xt\n",
            b"text\x1b\n",
            b"\x1b]0;t\x1bM\n",
        ] {
            assert_eq!(reach(moves), Reach::Anywhere, "{moves:?}");
        }
        assert_eq!(reach(b"text"), Reach::Forward);
        assert_eq!(reach(b"text\n\x1b[0m"), Reach::Below, "colour after");
    }

    /// **A line feed holds through what writes no cell, and not past what
    /// may draw**: after the feed, a colour, a `CR` and a tab, the shell's
    /// `C`, an iTerm badge reset, a link closed and a title leave the line
    /// out; an iTerm picture handed over whole or in parts, a sixel, a
    /// kitty picture or a `PM` keep it in, and so does a picture's body
    /// split between two reads. Named break: no control string draws --
    /// the pictures' lines are left out.
    #[test]
    fn a_line_feed_holds_through_what_writes_no_cell() {
        for clean in [
            &b"logout\r\n\x1b]1337;SetBadgeFormat=\x07"[..],
            b"% ls\r\n\x1b]133;C;baud_token=T\x07",
            b"x\n\x1b[0m\r\t\x1b]8;;\x1b\\\x1b]0;title\x07",
            b"x\n\x1b]1337;Fil\x07",
        ] {
            assert_eq!(reach(clean), Reach::Below, "{clean:?}");
        }
        for draws in [
            &b"x\n\x1b]1337;File=inline=1:AAAA\x07"[..],
            b"x\n\x1b]1337;MultipartFile=inline=1\x07",
            b"x\n\x1b]1337;FilePart=AAAA\x1b\\",
            b"x\n\x1bPq#0;2;0;0;0#0~\x1b\\",
            b"x\n\x1b_Ga=T;AAAA\x1b\\",
            b"x\n\x1b^private\x1b\\",
        ] {
            assert_eq!(reach(draws), Reach::Forward, "{draws:?}");
        }
        let mut r = Reads::default();
        let _ = r.read(b"x\n\x1b]1337;Fi");
        assert_eq!(r.read(b"le=:AAAA\x07"), Reach::Forward, "split");
    }

    /// **A sequence split between two reads is judged by its final byte**:
    /// `CSI 2` in one read and `A` then a line feed in the next keeps the
    /// second read's line in, where a fresh scan would read `A` as text;
    /// a control string split the same way stays a control string.
    /// Named break: start every read in text.
    #[test]
    fn a_split_sequence_is_judged_across_the_reads() {
        let mut r = Reads::default();
        assert_eq!(r.read(b"text\x1b[2"), Reach::Forward);
        assert_eq!(r.read(b"Ahi\r\n"), Reach::Anywhere);
        let mut r = Reads::default();
        assert_eq!(r.read(b"\x1b]7;file://h/a\x1bM"), Reach::Anywhere);
        let mut r = Reads::default();
        let _ = r.read(b"\x1b]7;file://h/a");
        assert_eq!(r.read(b"/b\x07out\r\n"), Reach::Below);
    }

    /// **Under `ESC % @` a byte from 0x80 to 0x9F is a control**: `9B A`,
    /// 8-bit `CSI A`, keeps the line in until `ESC % G`, after which the
    /// same bytes are part of a character. Named break: read them as text
    /// whatever the encoding.
    #[test]
    fn an_8_bit_control_under_latin_1_keeps_the_line_in() {
        let mut r = Reads::default();
        assert_eq!(r.read(b"\x1b%@"), Reach::Anywhere);
        assert_eq!(r.read(b"text\r\x9bA\n"), Reach::Anywhere);
        let _ = r.read(b"\x1b%G");
        assert_eq!(r.read(b"\xe2\x82\xac\n"), Reach::Below);
    }

    /// **A drain is below only when every read in it is, the last ending
    /// on a line feed**: one that moved up earlier may have written on the
    /// line the last ends on.
    #[test]
    fn a_drain_takes_the_reach_of_all_its_reads() {
        use Reach::*;
        assert_eq!(Forward.then(Below), Below);
        assert_eq!(Below.then(Forward), Forward);
        assert_eq!(Anywhere.then(Below), Anywhere);
        assert_eq!(Below.then(Anywhere), Anywhere);
    }

    /// **`ESC DEL` is still an escape across reads**, as the core reads it:
    /// a read ending `ESC DEL` and the next beginning `[1A`, the core's
    /// `CUU`, then text and a line feed keep the line in. A control after
    /// `ESC` goes on the same way. Named break: `DEL` after `ESC` back to
    /// ground -- the second read is text ending on a line feed, below.
    #[test]
    fn an_escape_goes_on_past_del_across_reads() {
        for first in [&b"x\x1b\x7f"[..], b"x\x1b\x07"] {
            let mut r = Reads::default();
            let _ = r.read(first);
            assert_eq!(r.read(b"[1Ax\r\n"), Reach::Anywhere, "{first:?}");
        }
    }

    /// **`ESC %` with anything before its final but a direct `@` or `G` is
    /// taken as `ESC % @` until `ESC % G`**, as the core may switch on the
    /// `@` after a control or a `DEL`: the scan reads 0x80-0x9F as controls
    /// and the read as one scanned under ISO 8859-1. Named break: `ESC %`
    /// and any byte but `@` or `G` back to ground under UTF-8 -- the
    /// encoding stays UTF-8.
    #[test]
    fn escape_percent_with_anything_before_its_final_fails_closed() {
        for shape in [&b"\x1b%\x07@"[..], b"\x1b%\x7f@", b"\x1b%\x01"] {
            let mut r = Reads::default();
            let _ = r.read(shape);
            assert_eq!(r.encoding(), Encoding::Latin1, "{shape:?}");
            assert_eq!(r.latin1_seen(), Latin1Seen::Yes, "{shape:?}");
            let _ = r.read(b"\x1b%G");
            assert_eq!(r.encoding(), Encoding::Utf8, "{shape:?}, then ESC % G");
        }
    }

    /// **A read any byte of which was scanned under `ESC % @` says so**,
    /// though it began and ended under UTF-8; the next read, all UTF-8,
    /// says not. Named break: say so only for the encoding as the read
    /// ended -- the read between says not.
    #[test]
    fn a_read_between_two_encodings_was_scanned_under_latin_1() {
        let mut r = Reads::default();
        let _ = r.read(b"\x1b%@\x9d7;x\x9c\x1b%G");
        assert_eq!(r.encoding(), Encoding::Utf8);
        assert_eq!(r.latin1_seen(), Latin1Seen::Yes, "the read between");
        let _ = r.read(b"text");
        assert_eq!(r.latin1_seen(), Latin1Seen::No, "the next read");
    }
}
