//! ReGIS, `DCS Pn p ... ST`: the body handed to `baud_domain::Regis` a
//! character at a time, and what it draws and answers wired to the
//! terminal.
//!
//! **The graph's `DCS` row routes a bare `p` here** (`Act::DcsQuery`, whose
//! byte tells it from DECRQSS's `$ q` and DECRSPS's `$ t`), after only the
//! numeric parameters a Sixel's `q` takes; `DCS $ p` and `DCS + p` keep their
//! own rows. `[[source: dec-std-070]]` 070-8 § 8.9.1: "ReGIS commands shall
//! be introduced by a DEC-private parameterized form of the ANSI escape
//! sequences Device Control String (DCS) and terminated the ANSI escape
//! sequence String Terminator (ST) ... the <protocol selector> for ReGIS is
//! lower case "p"."
//!
//! **The interpreter outlives the envelope**: an `ST`, a `CAN` or an `ESC`
//! that cancels the string leaves ReGIS where it was, and the next `DCS p`
//! resumes there unless its parameter's low bit resynchronises
//! (`RegisEntry`).

use crate::paint::Paint;
use crate::write::Csi;
use baud_domain::*;

impl Paint<'_, '_> {
    /// The string a `DcsQuery` byte other than DECRQSS's `q` begins:
    /// DECRSPS's `t` after `$`, or ReGIS's bare `p`. Out of line, so the
    /// per-byte loop's arm is DECRQSS's test and one call, the shape it had
    /// with DECRSPS's alone.
    #[cold]
    #[inline(never)]
    pub(crate) fn dcs_start(&mut self, b: StreamByte, csi: &Csi) -> crate::osc::StrKind {
        match b.raw_byte() {
            b'p' => self.regis_start(csi),
            _ => self.restore_start(csi),
        }
    }

    /// A `DCS Pn p`: enter ReGIS, `Pn` the envelope's first parameter.
    fn regis_start(&mut self, csi: &Csi) -> crate::osc::StrKind {
        let e = RegisEntry::of(Some(csi.params().raw(ParamIdx::FIRST, 0)));
        let (regis, mut out) = self.regis_parts();
        regis.enter(e, &mut out);
        crate::osc::StrKind::Regis
    }

    /// One body byte. **Under ISO 8859-1 a GR byte outside a quoted string
    /// is no ReGIS character**, the alphabet being "Columns 2 through 5 of
    /// the ASCII Code Table" (070-8 § 8.4.1), and the interpreter passes it
    /// over; inside one it is the 8-bit code § 8.4.3.2 allows, "10/0 through
    /// 15/15". The byte goes as it came.
    #[cold]
    #[inline(never)]
    pub(crate) fn regis_put(&mut self, b: StreamByte) {
        let (regis, mut out) = self.regis_parts();
        regis.put(b, &mut out);
    }

    /// The interpreter and what it draws and answers through, borrowed
    /// apart.
    fn regis_parts(&mut self) -> (&mut Regis, Out<'_>) {
        (
            &mut *self.strs.regis,
            Out {
                reply: &mut *self.reply,
                reply_len: &mut self.reply_len,
                overflows: &mut self.reply_overflows,
            },
        )
    }
}

/// What ReGIS draws on and answers through.
struct Out<'a> {
    reply: &'a mut [u8],
    reply_len: &'a mut ByteLen,
    overflows: &'a mut usize,
}

impl RegisOut for Out<'_> {
    fn line(&mut self, _: RegisPoint, _: RegisPoint, _: &RegisPen, _: &mut u32) {}
    fn fill(&mut self, _: &[RegisPoint], _: &[u16], _: &RegisPen) {}
    fn glyph(&mut self, _: &RegisGlyph<'_>, _: &RegisPen) {}
    fn erase(&mut self, _: RegisIndex) {}
    fn recolor(&mut self) {}

    /// **A report is sent whole or not at all**, the rule `want_reply`
    /// follows: a truncated one is a wrong answer, so one that does not fit
    /// is dropped and counted in `reply_overflows`.
    fn reply(&mut self, bytes: &[u8]) {
        let at = self.reply_len.index();
        match self.reply.get_mut(at..at + bytes.len()) {
            Some(dst) => {
                dst.copy_from_slice(bytes);
                *self.reply_len = ByteLen::from_index(at + bytes.len());
            }
            None => *self.overflows += 1,
        }
    }

    fn delay(&mut self, _: RegisTicks) {}
}
