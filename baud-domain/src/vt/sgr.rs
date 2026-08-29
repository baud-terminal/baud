use crate::*;

/// Which CSI parameters were introduced by a colon rather than a semicolon.
///
/// A bitset over [`ParamIdx`], not a `u16`. It is shifted when a parser hands
/// out a sub-slice of the parameters, and shifting the wrong quantity by the
/// wrong amount is exactly the mistake that would make `38:2:Pi:r:g:b` read as
/// the five-part form -- silently, and only for the second attribute in a
/// sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SubParams(pub(crate) u64);

// One bit per parameter, so the mask has to be at least as wide as the array.
const _: () = assert!(ParamIdx::CAPACITY <= u64::BITS as usize);

impl SubParams {
    pub const NONE: SubParams = SubParams(0);
    /// Mark parameter `i` as colon-joined to the one before it.
    pub fn mark(self, i: ParamIdx) -> SubParams {
        match i.raw_index() < ParamIdx::CAPACITY {
            true => SubParams(self.0 | (1 << i.raw_index())),
            false => self,
        }
    }
    pub fn is_sub(self, i: ParamIdx) -> bool {
        i.raw_index() < ParamIdx::CAPACITY && self.0 & (1 << i.raw_index()) != 0
    }
    /// Realign after dropping the first `k` parameters.
    pub fn drop_first(self, k: ParamIdx) -> SubParams {
        SubParams(self.0 >> k.raw_index())
    }
}

/// One resolved SGR attribute.
pub enum SgrAttr {
    Reset,
    Set(Attrs),
    Unset(Attrs),
    Fg(Color),
    Bg(Color),
    /// `SGR 10-19` — select the primary font or one of nine alternates.
    Font(FontId),
    /// `SGR 58` / `SGR 59` — the underline's own colour.
    ///
    /// `[[source: ctlseqs]]` gives it the same `5;n` and `2;r;g;b` spellings as
    /// the foreground and background, which is why it shares their parser.
    Underline(Color),
}

impl SgrAttr {
    /// Consumes as many parameters as the attribute needs and reports how many.
    /// Returning the consumed count is what keeps `38;2;r;g;b` from being
    /// misread as five separate attributes.
    pub fn parse(params: Params<'_>) -> Option<(SgrAttr, ParamCount)> {
        let v = params.at(ParamIdx::FIRST).map_or(0, |x| x.0);
        Some(match v {
            0 => (SgrAttr::Reset, ParamCount(1)),
            // `SGR 10-19`. Recorded rather than dropped: a font is a rendition,
            // and it lives in the rendition word's spare bits -- the row that
            // called this out of scope said fonts needed cell bits, and they
            // do not.
            10..=19 => (SgrAttr::Font(FontId::from_sgr(v)?), ParamCount(1)),
            1 => (SgrAttr::Set(Attrs::BOLD), ParamCount(1)),
            2 => (SgrAttr::Set(Attrs::DIM), ParamCount(1)),
            3 => (SgrAttr::Set(Attrs::ITALIC), ParamCount(1)),
            4 => (SgrAttr::Set(Attrs::UNDERLINE), ParamCount(1)),
            // 5 slow blink, 6 rapid blink. `[[source: ecma48]]` § 8.3.117
            // distinguishes them by rate; a terminal core has no timer and
            // nothing downstream can use the distinction, so both set BLINK.
            5 | 6 => (SgrAttr::Set(Attrs::BLINK), ParamCount(1)),
            7 => (SgrAttr::Set(Attrs::REVERSE), ParamCount(1)),
            8 => (SgrAttr::Set(Attrs::HIDDEN), ParamCount(1)),
            9 => (SgrAttr::Set(Attrs::STRIKE), ParamCount(1)),
            // 21 is doubly-underlined in ECMA-48 3rd and in xterm. baud has one
            // underline bit, so a double underline is an underline -- losing the
            // doubling is visibly wrong but losing the underline entirely is
            // worse, and inventing a second bit no renderer reads would be the
            // "set and never used" failure.
            21 => (SgrAttr::Set(Attrs::UNDERLINE), ParamCount(1)),
            22 => (SgrAttr::Unset(Attrs::BOLD.with(Attrs::DIM)), ParamCount(1)),
            20 => (SgrAttr::Set(Attrs::FRAKTUR), ParamCount(1)),
            // `[[source: ecma48]]` § 8.3.117 names 23 "not italicized, not
            // fraktur" -- one parameter clearing two attributes, so clearing
            // only italic would leave fraktur set with no way to turn it off.
            23 => (
                SgrAttr::Unset(Attrs::ITALIC.with(Attrs::FRAKTUR)),
                ParamCount(1),
            ),
            24 => (SgrAttr::Unset(Attrs::UNDERLINE), ParamCount(1)),
            25 => (SgrAttr::Unset(Attrs::BLINK), ParamCount(1)),
            27 => (SgrAttr::Unset(Attrs::REVERSE), ParamCount(1)),
            28 => (SgrAttr::Unset(Attrs::HIDDEN), ParamCount(1)),
            29 => (SgrAttr::Unset(Attrs::STRIKE), ParamCount(1)),
            30..=37 => (
                SgrAttr::Fg(Color::Indexed(Chan::from_offset(v, 30))),
                ParamCount(1),
            ),
            40..=47 => (
                SgrAttr::Bg(Color::Indexed(Chan::from_offset(v, 40))),
                ParamCount(1),
            ),
            39 => (SgrAttr::Fg(Color::Default), ParamCount(1)),
            49 => (SgrAttr::Bg(Color::Default), ParamCount(1)),
            // aixterm bright colours, `[[source: ctlseqs]]` § SGR: "aixterm
            // colors are the bright versions of the ISO colors". They are
            // indices 8..15 of the 256-colour palette, so they need no separate
            // representation -- 90 is index 8, not a new kind of colour.
            90..=97 => (
                SgrAttr::Fg(Color::Indexed(Chan::from_offset(v, 90 - 8))),
                ParamCount(1),
            ),
            100..=107 => (
                SgrAttr::Bg(Color::Indexed(Chan::from_offset(v, 100 - 8))),
                ParamCount(1),
            ),
            26 => (SgrAttr::Set(Attrs::PROPORTIONAL), ParamCount(1)),
            50 => (SgrAttr::Unset(Attrs::PROPORTIONAL), ParamCount(1)),
            51 => (SgrAttr::Set(Attrs::FRAMED), ParamCount(1)),
            52 => (SgrAttr::Set(Attrs::ENCIRCLED), ParamCount(1)),
            53 => (SgrAttr::Set(Attrs::OVERLINED), ParamCount(1)),
            // 54 clears both framed and encircled, as 23 clears two.
            54 => (
                SgrAttr::Unset(Attrs::FRAMED.with(Attrs::ENCIRCLED)),
                ParamCount(1),
            ),
            55 => (SgrAttr::Unset(Attrs::OVERLINED), ParamCount(1)),
            // `SGR 60-64` — the ideogram aspects, and `65` cancelling all five
            // at once. `[[source: ecma48]]` § 8.3.117 names each for two edges
            // ("ideogram underline or right side line") because which edge is
            // marked follows the writing direction; baud records the aspect
            // and leaves the edge to a renderer that knows the direction.
            //
            // 56-59 are not here: § 8.3.117 reserves 56, 57, 58 and 59 for
            // future standardization, and baud implements 58/59 as the
            // underline colour only because `[[source: ctlseqs]]` assigns them
            // that, which is an extension over ECMA-48 rather than a reading
            // of it. 56 and 57 nothing assigns, so nothing implements them.
            60 => (SgrAttr::Set(Attrs::IDEOGRAM_UNDERLINE), ParamCount(1)),
            61 => (
                SgrAttr::Set(Attrs::IDEOGRAM_DOUBLE_UNDERLINE),
                ParamCount(1),
            ),
            62 => (SgrAttr::Set(Attrs::IDEOGRAM_OVERLINE), ParamCount(1)),
            63 => (SgrAttr::Set(Attrs::IDEOGRAM_DOUBLE_OVERLINE), ParamCount(1)),
            64 => (SgrAttr::Set(Attrs::IDEOGRAM_STRESS), ParamCount(1)),
            65 => (SgrAttr::Unset(Attrs::IDEOGRAM), ParamCount(1)),
            // `SGR 59` — underline colour back to the default, which is
            // "follow the foreground".
            59 => (SgrAttr::Underline(Color::Default), ParamCount(1)),
            // `SGR 58` takes the same two spellings as 38 and 48, which is why
            // it joins them here rather than getting a parallel copy: the
            // colon/semicolon distinction below is the part that is easy to
            // get wrong, and one copy of it cannot drift from another.
            38 | 48 | 58 => {
                let mk = |c| match v {
                    38 => SgrAttr::Fg(c),
                    48 => SgrAttr::Bg(c),
                    _ => SgrAttr::Underline(c),
                };
                // Two spellings, and the difference is not cosmetic.
                //
                // `[[source: ctlseqs]]` gives the ISO 8613-6 form as
                // `38 : 2 : Pi : Pr : Pg : Pb` -- six sub-parameters, with a
                // colour-space identifier `Pi` that "is ignored" -- and the
                // konsole-compatible legacy form as `38 ; 2 ; Pr ; Pg ; Pb`,
                // five parameters with no `Pi`. Reading the colon form as the
                // semicolon form takes `Pi` for red and shifts every channel
                // by one, which is the bug this whole distinction exists to
                // avoid. Some emitters also write `38:2:r:g:b` with no `Pi`,
                // so the group length decides rather than the separator alone.
                let group = params.group_len(ParamIdx::FIRST);
                let colon_form = group > 1;
                match params.at(ParamIdx::SECOND).map(|x| x.0) {
                    Some(5) => {
                        let n = mk(Color::Indexed(Chan::from_param(params.at(ParamIdx::THIRD))));
                        // The whole colon group is one attribute, however long
                        // the emitter made it.
                        (n, ParamCount(if colon_form { group } else { 3 }))
                    }
                    Some(2) => {
                        // Where the channels start: after `Pi` in the six-part
                        // colon form, immediately in every other spelling.
                        let base = if colon_form && group >= 6 {
                            ParamIdx::FOURTH
                        } else {
                            ParamIdx::THIRD
                        };
                        let c = mk(Color::Rgb(
                            Chan::from_param(params.at(base)),
                            Chan::from_param(params.at(base.plus(1))),
                            Chan::from_param(params.at(base.plus(2))),
                        ));
                        (c, ParamCount(if colon_form { group } else { 5 }))
                    }
                    _ => return None,
                }
            }
            _ => return None,
        })
    }
}

impl SgrAttr {
    /// Parse the attribute starting at `i`.
    pub fn parse_at(params: &[Option<Param>], i: ParamIdx) -> Option<(SgrAttr, ParamCount)> {
        SgrAttr::parse(Params::new(&params[i.0 as usize..]))
    }
    /// As [`SgrAttr::parse_at`], but keeping the sub-parameter mask aligned
    /// with the slice. Dropping the mask here would make every attribute after
    /// the first look like the legacy form.
    pub fn parse_sub(params: Params<'_>, i: ParamIdx) -> Option<(SgrAttr, ParamCount)> {
        SgrAttr::parse(params.skip(i))
    }
    /// Apply to a packed 24-bit colour + attribute word.
    /// Apply one attribute to the accumulating rendition.
    ///
    /// Every variant is handled. It used to fall through to `cur.bits()` for
    /// `Set`, `Unset` and every `Bg`, so bold, italic, underline, reverse,
    /// blink, conceal, strike and all background colours were parsed and then
    /// silently discarded.
    /// Fold this attribute into `cur` **in place**.
    ///
    /// [`SgrAttr::fold`] copies the twenty-byte rendition in and out to change
    /// four bytes of it, and the caller then assigns the result back. SGR
    /// handling is roughly half of `asciiquarium.raw` by `opcost`, and one
    /// attribute was moving sixty bytes to set one field.
    #[inline(always)]
    pub fn fold_into(self, cur: &mut StyleWord) {
        match self {
            SgrAttr::Reset => *cur = StyleWord::DEFAULT,
            SgrAttr::Set(a) => cur.set_attrs(cur.attrs().with(a)),
            SgrAttr::Unset(a) => cur.set_attrs(cur.attrs().without(a)),
            SgrAttr::Fg(c) => cur.set_fg(ColorWord::from_color(c)),
            SgrAttr::Bg(c) => cur.set_bg(ColorWord::from_color(c)),
            SgrAttr::Font(f) => cur.set_font(f),
            SgrAttr::Underline(c) => cur.set_underline(ColorWord::from_color(c)),
        }
    }

    pub fn fold(self, cur: StyleWord) -> StyleWord {
        match self {
            // `[[source: ecma48]]` § 8.3.117: SGR 0 "cancels the effect of any
            // preceding occurrence of SGR in the data stream".
            SgrAttr::Reset => StyleWord::DEFAULT,
            SgrAttr::Set(a) => cur.with_attrs(cur.attrs().with(a)),
            SgrAttr::Unset(a) => cur.with_attrs(cur.attrs().without(a)),
            SgrAttr::Fg(c) => cur.with_fg(ColorWord::from_color(c)),
            SgrAttr::Bg(c) => cur.with_bg(ColorWord::from_color(c)),
            SgrAttr::Font(f) => cur.with_font(f),
            SgrAttr::Underline(c) => cur.with_underline(ColorWord::from_color(c)),
        }
    }
}
