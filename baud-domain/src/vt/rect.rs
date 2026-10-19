//! The VT420 rectangular area operations: their rectangle, their fill
//! character, their attribute changes and the checksum DECRQCRA reports.
//!
//! `[[source: dec-std-070]]` § 5.12.1 and § 5.12.2 (pp. 5-168 to 5-181),
//! with `[[source: vt420-rm]]` chapter 9 and the `vt510-dec*` pages.

use crate::*;

/// A rectangle as the rectangle operations spell it, `Pt ; Pl ; Pb ; Pr`.
///
/// **An omitted or 0 parameter is its default**: `Pt` and `Pl` are 1, `Pb`
/// and `Pr` "last-line-of-page" and "last-column-of-page" (`[[source:
/// dec-std-070]]` DECCRA to DECRARA, pp. 5-169 to 5-175; § 3.5.3.1, "A zero
/// length parameter, or one consisting only of 3/0 bit combinations,
/// represents a default"). `None` is that page edge, as
/// [`Params::far_margin`] reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Area {
    pub top: OneBased,
    pub left: OneBased,
    pub bottom: Option<OneBased>,
    pub right: Option<OneBased>,
}

impl Area {
    /// The four parameters from `first` on.
    pub fn from_params(p: &Params<'_>, first: ParamIdx) -> Area {
        Area {
            top: p.one_based(first),
            left: p.one_based(first.plus(1)),
            bottom: p.far_margin(first.plus(2)),
            right: p.far_margin(first.plus(3)),
        }
    }
}

/// Which rectangle operation a `CSI` named, by its intermediate and final.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AreaOp {
    /// DECCRA, `CSI Pts ; Pls ; Pbs ; Prs ; Pps ; Ptd ; Pld ; Ppd $ v`.
    Copy,
    /// DECFRA, `CSI Pch ; Pt ; Pl ; Pb ; Pr $ x`.
    Fill,
    /// DECERA, `CSI Pt ; Pl ; Pb ; Pr $ z`.
    Erase,
    /// DECSERA, `CSI Pt ; Pl ; Pb ; Pr $ {`.
    SelectiveErase,
    /// DECCARA, `CSI Pt ; Pl ; Pb ; Pr ; Ps... $ r`.
    ChangeAttrs,
    /// DECRARA, `CSI Pt ; Pl ; Pb ; Pr ; Ps... $ t`.
    ReverseAttrs,
    /// DECSACE, `CSI Ps * x`.
    SelectExtent,
    /// DECRQCRA, `CSI Pid ; Pp ; Pt ; Pl ; Pb ; Pr * y`.
    RequestChecksum,
}

/// A rectangle operation with its parameters read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AreaCall {
    /// Copy the rectangle `from` to `to_top`, `to_left`. The page
    /// parameters name baud's one page whatever they say: "If a page value
    /// exceeds the number of pages available in the current Page
    /// Configuration, it is treated as the last page" (`[[source:
    /// dec-std-070]]` p. 5-169).
    Copy {
        from: Area,
        to_top: OneBased,
        to_left: OneBased,
    },
    /// `fill` is `None` for a `Pch` outside the text's ranges, a command it
    /// ignores.
    Fill {
        fill: Option<FillChar>,
        area: Area,
    },
    Erase(Area),
    SelectiveErase(Area),
    /// The attributes are the parameters from the fifth on
    /// ([`AreaAttrs::apply`]).
    Attrs(Area, AreaAttrs),
    /// `None` for a value the text does not give.
    SelectExtent(Option<AttrExtent>),
    /// Answered with [`Report::AreaChecksum`].
    RequestChecksum {
        id: Param,
        of: ChecksumOf,
    },
}

impl AreaOp {
    /// The operation with its parameters, `p`.
    pub fn with(self, p: &Params<'_>) -> AreaCall {
        match self {
            AreaOp::Copy => AreaCall::Copy {
                from: Area::from_params(p, ParamIdx::FIRST),
                to_top: p.one_based(ParamIdx::at(5)),
                to_left: p.one_based(ParamIdx::at(6)),
            },
            AreaOp::Fill => AreaCall::Fill {
                fill: FillChar::from_param(p.at(ParamIdx::FIRST)),
                area: Area::from_params(p, ParamIdx::SECOND),
            },
            AreaOp::Erase => AreaCall::Erase(Area::from_params(p, ParamIdx::FIRST)),
            AreaOp::SelectiveErase => {
                AreaCall::SelectiveErase(Area::from_params(p, ParamIdx::FIRST))
            }
            AreaOp::ChangeAttrs => {
                AreaCall::Attrs(Area::from_params(p, ParamIdx::FIRST), AreaAttrs::Change)
            }
            AreaOp::ReverseAttrs => {
                AreaCall::Attrs(Area::from_params(p, ParamIdx::FIRST), AreaAttrs::Reverse)
            }
            AreaOp::SelectExtent => {
                AreaCall::SelectExtent(AttrExtent::from_param(p.at(ParamIdx::FIRST)))
            }
            AreaOp::RequestChecksum => AreaCall::RequestChecksum {
                id: p.at(ParamIdx::FIRST).unwrap_or(Param::ZERO),
                of: match p.raw(ParamIdx::SECOND, 0) {
                    0 => ChecksumOf::Page,
                    _ => ChecksumOf::Area(Area::from_params(p, ParamIdx::THIRD)),
                },
            },
        }
    }
}

/// A rectangle on the page: 0-based rows and columns, both ends included.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageRect {
    pub top: Row,
    pub left: Col,
    pub bottom: Row,
    pub right: Col,
}

impl PageRect {
    /// How many columns wide.
    pub fn cols(self) -> Cols {
        Cols::of_index(self.right.index() + 1 - self.left.index())
    }
    /// How many rows tall.
    pub fn rows(self) -> Rows {
        Rows::of_count(self.bottom.index() + 1 - self.top.index())
    }
}

/// A row parameter from the origin row `origin`, clamped to the page's last.
pub fn page_row(v: OneBased, origin: Row, rows: Rows) -> Row {
    let at = origin.index() + usize::from(v.get()) - 1;
    Row::from_index(at.min(rows.index().saturating_sub(1)))
}

/// A column parameter from the origin column `origin`, clamped to the
/// page's last.
pub fn page_col(v: OneBased, origin: Col, cols: Cols) -> Col {
    let at = origin.index() + usize::from(v.get()) - 1;
    Col::at_index(at.min(cols.index().saturating_sub(1)))
}

impl Area {
    /// The rectangle on a page `rows` by `cols` whose origin is `top`,
    /// `left`: the home position, or DECOM's top and left margins.
    ///
    /// **From the origin, and clamped to the page, not to the margins**:
    /// "The coordinates of the rectangular area are affected by the setting
    /// of Origin Mode. This control is not otherwise affected by the
    /// margins", and "If a value exceeds the width or height of the active
    /// page, it is treated as the width or height of the active page"
    /// (`[[source: dec-std-070]]` pp. 5-169 to 5-175). xterm clamps into the
    /// margins under DECOM (`limitedParseRow`, `[[source: xterm-screen]]`),
    /// a departure. `None` where the top is below the bottom or the left
    /// right of the right, "otherwise the entire control function is
    /// ignored", compared once both are on the page, as xterm's `validRect`
    /// compares them.
    pub fn on_page(self, top: Row, left: Col, rows: Rows, cols: Cols) -> Option<PageRect> {
        let r = PageRect {
            top: page_row(self.top, top, rows),
            left: page_col(self.left, left, cols),
            bottom: self
                .bottom
                .map_or(Row::from_index(rows.index().saturating_sub(1)), |v| {
                    page_row(v, top, rows)
                }),
            right: self
                .right
                .map_or(Col::at_index(cols.index().saturating_sub(1)), |v| {
                    page_col(v, left, cols)
                }),
        };
        (r.top <= r.bottom && r.left <= r.right).then_some(r)
    }
}

/// DECFRA's fill character, `Pch`: "a decimal value from 32 to 126, or from
/// 160 to 255 ... If the value of Pch is not in the above range, the command
/// is ignored" (`[[source: dec-std-070]]` p. 5-170). Omitted or 0, it is the
/// default, 32.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FillChar(u8);

impl FillChar {
    pub fn from_param(p: Option<Param>) -> Option<FillChar> {
        match p.map_or(0, |v| v.0) {
            0 => Some(FillChar(b' ')),
            v @ (32..=126 | 160..=255) => Some(FillChar(v as u8)),
            _ => None,
        }
    }
    /// A GL value, `32-126`: the byte the current GL set translates.
    pub fn gl(self) -> Option<StreamByte> {
        (self.0 < 0x80).then(|| StreamByte::from_stream(self.0))
    }
    /// The character a GR value, `160-255`, names: the Latin-1 supplement,
    /// U+00A0 + (Pch - 160), under either input encoding (decided by Jev
    /// 2026-10-18, LATIN1, 0.92 and 0.98): it is the GR set of an ISO 8859-1
    /// terminal, and under UTF-8 the same characters.
    pub fn gr(self) -> Option<Codepoint> {
        (self.0 >= 0xa0).then(|| Codepoint(u32::from(self.0)))
    }
}

/// DECSACE, `CSI Ps * x`: which positions DECCARA and DECRARA affect.
///
/// `[[source: dec-std-070]]` p. 5-177: "Ps = 0 stream of character positions
/// are affected (default) / Ps = 1 stream ... / Ps = 2 rectangular area".
/// STD 070's host_state_init sets it to `STREAM` (070-4 p. 15, PDF 219), and DECSTR's
/// table (p. 31, PDF 235) does not list it, so only RIS puts it back.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AttrExtent {
    /// The positions from the first corner to the second, wrapping at the
    /// page's edges; unoccupied positions are not affected.
    #[default]
    Stream,
    /// Every position in the rectangle; an unoccupied one becomes a blank
    /// and is affected.
    Rectangle,
}

impl AttrExtent {
    /// `None` for a value the text does not give.
    pub fn from_param(p: Option<Param>) -> Option<AttrExtent> {
        match p.map_or(0, |v| v.0) {
            0 | 1 => Some(AttrExtent::Stream),
            2 => Some(AttrExtent::Rectangle),
            _ => None,
        }
    }
    /// The `Ps` DECRQSS reports.
    pub fn wire(self) -> u8 {
        match self {
            AttrExtent::Stream => 0,
            AttrExtent::Rectangle => 2,
        }
    }
}

/// What DECRQCRA asks the checksum of: `Pp` 0 or omitted is "all page
/// memory" and the rectangle is ignored (`[[source: dec-std-070]]` p.
/// 5-179); any other page is baud's one page, "If Pp is higher than the total
/// number of pages available in the device, the highest numbered page is
/// used".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChecksumOf {
    Page,
    Area(Area),
}

impl Attrs {
    /// The aspects DECCARA and DECRARA reach: "bold, blink, underline, and
    /// reverse" (`[[source: dec-std-070]]` p. 5-173). Faint rides with bold
    /// because DECCARA's 22 is "Normal intensity", as SGR 22 is.
    const AREA: Attrs = Attrs(
        Attrs::BOLD.0 | Attrs::DIM.0 | Attrs::UNDERLINES.0 | Attrs::BLINKS.0 | Attrs::REVERSE.0,
    );

    /// One DECCARA parameter applied, or `None` for a value it ignores.
    ///
    /// **Each does what its SGR namesake does** (Jev 2026-10-18, AS_SGR,
    /// 0.70 and 0.88, the lean taken): "These parameters correspond to the
    /// parameters of the Set Graphic Rendition control function (SGR)", so
    /// 22 clears faint too, 24 every underline style and 25 both blinks, and
    /// 4 and 5 replace their families. 0 is "Attributes off (normal
    /// intensity, positive image, not underlined, not blinking)" and nothing
    /// more: not SGR 0, which would reset the colours. "All other parameter
    /// values shall be ignored" -- 8 and 28 too, xterm's extension.
    pub fn changed_by(self, p: Param) -> Option<Attrs> {
        Some(match p.0 {
            0 => self.without(Attrs::AREA),
            1 => self.with(Attrs::BOLD),
            4 => self.without(Attrs::UNDERLINES).with(Attrs::UNDERLINE),
            5 => self.without(Attrs::BLINKS).with(Attrs::BLINK),
            7 => self.with(Attrs::REVERSE),
            22 => self.without(Attrs::BOLD.with(Attrs::DIM)),
            24 => self.without(Attrs::UNDERLINES),
            25 => self.without(Attrs::BLINKS),
            27 => self.without(Attrs::REVERSE),
            _ => return None,
        })
    }

    /// One DECRARA parameter applied, or `None` for a value it ignores.
    ///
    /// "To reverse the attribute bit ... means to set the specified attribute
    /// bits if it was not previously set, and reset the bit(s) if it was
    /// already set" (`[[source: dec-std-070]]` p. 5-175). An aspect is set
    /// when any of its family is in force: a curly underline reversed is no
    /// underline, and no underline reversed is a single one (AS_SGR). Bold
    /// reverses alone; faint is not one of the four.
    pub fn reversed_by(self, p: Param) -> Option<Attrs> {
        let flip = |a: Attrs, family: Attrs, one: Attrs| match a.0 & family.0 != 0 {
            true => a.without(family),
            false => a.with(one),
        };
        Some(match p.0 {
            0 => {
                let a = flip(self, Attrs::BOLD, Attrs::BOLD);
                let a = flip(a, Attrs::UNDERLINES, Attrs::UNDERLINE);
                let a = flip(a, Attrs::BLINKS, Attrs::BLINK);
                flip(a, Attrs::REVERSE, Attrs::REVERSE)
            }
            1 => flip(self, Attrs::BOLD, Attrs::BOLD),
            4 => flip(self, Attrs::UNDERLINES, Attrs::UNDERLINE),
            5 => flip(self, Attrs::BLINKS, Attrs::BLINK),
            7 => flip(self, Attrs::REVERSE, Attrs::REVERSE),
            _ => return None,
        })
    }
}

/// DECCARA or DECRARA, for the attribute parameters from the fifth on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AreaAttrs {
    /// DECCARA: each parameter sets or clears ([`Attrs::changed_by`]). "The
    /// default for the video attribute parameters, if no valid subsequent
    /// parameter is received, is 0", and an omitted one is that default.
    Change,
    /// DECRARA: each parameter reverses ([`Attrs::reversed_by`]). "There is
    /// no default ... if no valid subsequent parameter is received, the
    /// command is ignored" (`[[source: dec-std-070]]` p. 5-176), against the
    /// VT420 and VT510 manuals' "0 Reverse all attributes (default)": the
    /// standard, decided by Jev 2026-10-18 (IGNORE, 0.94 and 0.98).
    Reverse,
}

impl AreaAttrs {
    const FIRST: usize = 4;

    /// Whether the command acts at all: DECCARA always, DECRARA only with a
    /// parameter it reverses.
    pub fn acts(self, p: &Params<'_>) -> bool {
        match self {
            AreaAttrs::Change => true,
            AreaAttrs::Reverse => (AreaAttrs::FIRST..p.len()).any(|i| {
                p.at(ParamIdx::at(i))
                    .is_some_and(|v| Attrs::NONE.reversed_by(v).is_some())
            }),
        }
    }

    /// `a` with every attribute parameter applied in turn: "When multiple
    /// parameters are used, they are cumulative, as if several DECCARA
    /// control functions were received and executed in sequence".
    pub fn apply(self, p: &Params<'_>, a: Attrs) -> Attrs {
        let (mut out, mut valid) = (a, false);
        for i in AreaAttrs::FIRST..p.len() {
            let v = p.at(ParamIdx::at(i));
            let next = match self {
                AreaAttrs::Change => out.changed_by(v.unwrap_or(Param::ZERO)),
                AreaAttrs::Reverse => v.and_then(|v| out.reversed_by(v)),
            };
            if let Some(n) = next {
                out = n;
                valid = true;
            }
        }
        match (self, valid) {
            (AreaAttrs::Change, false) => a.changed_by(Param::ZERO).unwrap_or(a),
            _ => out,
        }
    }
}

/// The checksum DECRQCRA reports, accumulated a cell at a time.
///
/// **No DEC text defines the algorithm**: DEC STD 070 asks for "a checksum"
/// in "four hexadecimal digits" (pp. 5-179, 5-181) and the VT420 and VT510
/// references say no more. This is xterm's `csDEC` mode
/// (`[[source: xterm-screen]]`, `xtermCheckRect`), the one vttest's
/// expected values agree with: each written cell's 8-bit code plus 0x80 for
/// bold, 0x40 blink, 0x20 reverse, 0x10 underline, 0x08 invisible and 0x04
/// protected; an unwritten cell counted not at all; the sum negated, sixteen
/// bits. A written space counts like any other character: xterm sets
/// `CHARDRAWN` on every cell it writes, and its trim test passes any cell
/// carrying that bit. baud's extra aspects are weighted by family: any
/// underline style adds 0x10 and either blink 0x40, where xterm's `csDEC`
/// adds 0x10 only for its single-underline bit (SGR 21 sets
/// `ATR_DBL_UNDER`, which it does not count). Answering at all is
/// Jev's lean of 2026-10-18 (ANSWER_ALWAYS, 0.49 against 0.47 for an
/// embedder opt-in, after a second ask).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Checksum {
    sum: u16,
}

impl Default for Checksum {
    fn default() -> Checksum {
        Checksum::new()
    }
}

impl Checksum {
    pub const fn new() -> Checksum {
        Checksum { sum: 0 }
    }

    /// Count one cell, whose base character is `base`.
    ///
    /// **A character's code is its 8-bit value**: the DEC Special Graphics
    /// byte for a cell written under that set (xterm counts the byte it
    /// received there, `charSeen`), the codepoint up to U+00FF, and 0x1B, the
    /// value xterm's `xtermCharSetDec` gives, for anything above. A wide
    /// character's tail is not a character and is not counted; combining
    /// marks are not either, as `csDEC` leaves them out.
    pub fn add(&mut self, base: Content, flags: CellFlags, attrs: Attrs) {
        if base.is_empty() || base.is_wide_tail() {
            return;
        }
        let code = match base.as_codepoint() {
            Some(cp) if flags.is_graphics() => match dec_graphics_byte(cp) {
                Some(b) => u16::from(b),
                None => Checksum::eight_bit(cp),
            },
            Some(cp) => Checksum::eight_bit(cp),
            None => 0x1b,
        };
        let weight = |on: bool, w: u16| if on { w } else { 0 };
        let ch = code
            + weight(attrs.has(Attrs::BOLD), 0x80)
            + weight(attrs.0 & Attrs::BLINKS.0 != 0, 0x40)
            + weight(attrs.has(Attrs::REVERSE), 0x20)
            + weight(attrs.0 & Attrs::UNDERLINES.0 != 0, 0x10)
            + weight(attrs.has(Attrs::HIDDEN), 0x08)
            + weight(flags.is_protected(), 0x04);
        self.sum = self.sum.wrapping_add(ch);
    }

    fn eight_bit(cp: Codepoint) -> u16 {
        match u16::try_from(cp.0) {
            Ok(v) if v <= 0xff => v,
            _ => 0x1b,
        }
    }

    /// The sixteen bits the report carries: the sum, negated.
    pub fn value(self) -> u16 {
        self.sum.wrapping_neg()
    }
}

/// DEC Special Graphics backwards, codepoint to byte, for the checksum of a
/// cell written under that set: a table built from [`DEC_GRAPHICS`] at
/// compile time and keyed on the codepoint modulo 127, the smallest modulus
/// that gives each of the 31 codepoints a slot of its own (the build fails if
/// two ever share one).
static DEC_GRAPHICS_BYTE: [(u32, u8); 127] = dec_graphics_reversed();

const fn dec_graphics_reversed() -> [(u32, u8); 127] {
    let mut t = [(0u32, 0u8); 127];
    let mut i = 0;
    while i < DEC_GRAPHICS.len() {
        let cp = DEC_GRAPHICS[i];
        let k = (cp % 127) as usize;
        assert!(
            t[k].0 == 0,
            "two DEC Special Graphics codepoints share a slot"
        );
        t[k] = (cp, 0x60 + i as u8);
        i += 1;
    }
    t
}

/// The byte a DEC Special Graphics cell was written with, if `cp` is one the
/// set translates to.
fn dec_graphics_byte(cp: Codepoint) -> Option<u8> {
    let (c, b) = DEC_GRAPHICS_BYTE[(cp.0 % 127) as usize];
    (c == cp.0 && c != 0).then_some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reversed table names every byte the forward one translates, and
    /// nothing else.
    #[test]
    fn dec_graphics_reverses_to_the_byte_it_came_from() {
        for (i, &cp) in DEC_GRAPHICS.iter().enumerate() {
            assert_eq!(dec_graphics_byte(Codepoint(cp)), Some(0x60 + i as u8));
        }
        assert_eq!(dec_graphics_byte(Codepoint(u32::from(b'A'))), None);
        assert_eq!(dec_graphics_byte(Codepoint(0)), None);
    }

    /// DECCARA's parameters each do what the same SGR parameter does to the
    /// attributes, 0 aside: it is "Attributes off" for the four aspects, not
    /// SGR 0's reset of the colours too.
    #[test]
    fn deccara_parameters_act_as_their_sgr_namesakes() {
        let starts = [
            Attrs::NONE,
            Attrs::BOLD.with(Attrs::DIM),
            Attrs::CURLY_UNDERLINE.with(Attrs::RAPID_BLINK),
            Attrs::REVERSE.with(Attrs::ITALIC),
        ];
        for p in [1u16, 4, 5, 7, 22, 24, 25, 27] {
            let Some((sgr, _)) = SgrAttr::parse(Params::new(&[Some(Param(p))])) else {
                panic!("SGR {p} parses");
            };
            for a in starts {
                let want = sgr.fold(StyleWord::DEFAULT.with_attrs(a)).attrs();
                assert_eq!(a.changed_by(Param(p)), Some(want), "DECCARA {p} on {a:?}");
            }
        }
        let all = Attrs::BOLD.with(Attrs::DIM).with(Attrs::DOTTED_UNDERLINE);
        let all = all
            .with(Attrs::BLINK)
            .with(Attrs::REVERSE)
            .with(Attrs::ITALIC);
        assert_eq!(all.changed_by(Param(0)), Some(Attrs::ITALIC));
        for p in [2u16, 3, 6, 8, 9, 28, 30, 38] {
            assert_eq!(
                Attrs::NONE.changed_by(Param(p)),
                None,
                "DECCARA ignores {p}"
            );
        }
    }

    /// DECRARA reverses an aspect by whether any of its family is in force.
    #[test]
    fn decrara_reverses_whole_families() {
        let curly = Attrs::CURLY_UNDERLINE;
        assert_eq!(curly.reversed_by(Param(4)), Some(Attrs::NONE));
        assert_eq!(Attrs::NONE.reversed_by(Param(4)), Some(Attrs::UNDERLINE));
        assert_eq!(Attrs::RAPID_BLINK.reversed_by(Param(5)), Some(Attrs::NONE));
        assert_eq!(
            Attrs::DIM.reversed_by(Param(1)),
            Some(Attrs::DIM.with(Attrs::BOLD))
        );
        let all = Attrs::BOLD
            .with(Attrs::UNDERLINE)
            .with(Attrs::BLINK)
            .with(Attrs::REVERSE);
        assert_eq!(all.reversed_by(Param(0)), Some(Attrs::NONE));
        assert_eq!(Attrs::NONE.reversed_by(Param(0)), Some(all));
        assert_eq!(Attrs::NONE.reversed_by(Param(8)), None);
    }

    /// vttest's expectation for one cell: `CHK(n) = (-(n)) & 0xffff`.
    #[test]
    fn a_checksum_is_the_negated_sum_of_codes_and_weights() {
        let mut c = Checksum::new();
        c.add(
            Content::codepoint(Codepoint(u32::from(b'A'))),
            CellFlags::NONE,
            Attrs::NONE,
        );
        assert_eq!(c.value(), 0x41u16.wrapping_neg());
        c.add(
            Content::codepoint(Codepoint(u32::from(b' '))),
            CellFlags::NONE,
            Attrs::NONE,
        );
        assert_eq!(
            c.value(),
            (0x41u16 + 0x20).wrapping_neg(),
            "a written space counts"
        );
        c.add(
            Content::codepoint(Codepoint(u32::from(b' '))),
            CellFlags::PROTECTED,
            Attrs::BOLD,
        );
        assert_eq!(
            c.value(),
            (0x41u16 + 0x20 + 0x20 + 0x80 + 0x04).wrapping_neg()
        );
        let mut first = Checksum::new();
        first.add(Content::EMPTY, CellFlags::NONE, Attrs::NONE);
        first.add(
            Content::codepoint(Codepoint(u32::from(b' '))),
            CellFlags::NONE,
            Attrs::NONE,
        );
        assert_eq!(
            first.value(),
            0x20u16.wrapping_neg(),
            "an unwritten cell counts nothing"
        );
        let mut g = Checksum::new();
        g.add(
            Content::codepoint(Codepoint(0x2500)),
            CellFlags::GRAPHICS,
            Attrs::NONE,
        );
        assert_eq!(g.value(), u16::from(b'q').wrapping_neg());
        let mut w = Checksum::new();
        w.add(
            Content::codepoint(Codepoint(0x4e2d)),
            CellFlags::NONE,
            Attrs::NONE,
        );
        w.add(Content::WIDE_TAIL, CellFlags::NONE, Attrs::NONE);
        assert_eq!(w.value(), 0x1bu16.wrapping_neg());
    }
}
