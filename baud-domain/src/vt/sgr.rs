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
///
/// `Copy` so the single-parameter table can be a `const` array of them: every
/// variant is a small value already.
#[derive(Clone, Copy, PartialEq, Eq)]
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
    /// The underline's style, one of [`Attrs::UNDERLINES`] or `NONE`: what
    /// `SGR 4:n` builds, and a bare `4` (`4:1`) and `21` (`4:2`) too.
    /// **Replaces the family rather than setting a bit**: a `4:3` after a
    /// `4:2` or a `21` is curly, not double-and-curly, because exactly one
    /// style is in force at a time.
    UnderlineStyle(Attrs),
}

impl SgrAttr {
    /// Is the parameter mask empty -- no colon-joined groups anywhere?
    ///
    /// The gate on [`SgrAttr::parse_one`]: a colon form can make a single
    /// parameter part of a longer group, and the fast path must not see one.
    #[inline(always)]
    pub fn is_plain(sub: SubParams) -> bool {
        sub.0 == 0
    }

    /// How many single-parameter `SGR` values the table covers. Above this
    /// the parameter is a colour introducer or out of range, and the general
    /// grammar handles it.
    pub const ONE_MAX: usize = 128;

    /// One parameter, no colon groups, and not one of the multi-parameter
    /// colour introducers.
    ///
    /// `CSI 32 m`, `CSI 0 m`, `CSI 1 m` -- what a sixteen-colour program emits,
    /// and **86%** of `asciiquarium.raw`'s SGRs. `apply_sgr` reaches this
    /// without building a `Params` at all, which is the whole point: `opcost`
    /// prices one `SgrAttr::parse` at **3.3 ns** of SGR's ~10, and most of a
    /// single-parameter parse is arriving at the value.
    ///
    /// Returns `None` for `38`, `48` and `58`, which consume following
    /// parameters and need the general path, and for anything this does not
    /// recognise -- so the caller falls back rather than dropping the
    /// attribute.
    ///
    /// The attributes are **a table**, and this is the indexed read of it.
    ///
    /// **The commonest control sequence in every recording is one parameter
    /// long** -- `ESC [ 0 m`, `ESC [ 22 m`, `ESC [ 32 m` -- and answering it
    /// meant walking a chain of range tests. Bounded input, pure function,
    /// so it is a lookup: 128 entries built at compile time.
    ///
    /// `the_one_parameter_table_agrees_with_the_grammar` checks every entry
    /// against `parse`, so the table cannot mean something the grammar does
    /// not.
    ///
    /// **This function had an `#[inline(always)]` that was never applied and
    /// should not be.** It was written for `one` and an insertion pushed
    /// `ONE_MAX` between the two, so it decorated an associated const, where
    /// it does nothing and where rustc warns it will become a hard error.
    /// Re-attaching it is a measured *loss* -- btop 883 to 870, vim 706 to
    /// 689, same direction over three rounds -- because `apply_sgr` is already
    /// inlined into the per-byte body and this only makes that body bigger.
    /// So the attribute is gone rather than moved, and the placement rule this
    /// repo keeps re-learning applies to finding it: after any edit near an
    /// annotated function, check which signature follows the attribute.
    pub fn one(v: Param) -> Option<SgrAttr> {
        ONE.get(v.index()).copied().flatten()
    }

    /// **Deliberately not `#[inline]`, and that is measured.** It builds an
    /// `SgrAttr` that `fold_into` -- already `#[inline(always)]` --
    /// destructures immediately, so threading the two together looked like the
    /// same win `Csi::parse_with` gets. It is a *loss*: btop 937 to 924,
    /// asciiquarium 590 to 581, less-pager 1729 to 1708. `parse` is a large
    /// match and `fold_sgr` runs once per `SGR`, so growing it costs more than
    /// collapsing the pair saves. The difference from `Csi::op` is the caller:
    /// `csi_final` is `#[cold]` and can absorb the growth, `fold_sgr` cannot.
    pub fn parse_one(v: u16) -> Option<SgrAttr> {
        Some(match v {
            30..=37 => SgrAttr::Fg(Color::Indexed(Chan::from_offset(v, 30))),
            40..=47 => SgrAttr::Bg(Color::Indexed(Chan::from_offset(v, 40))),
            90..=97 => SgrAttr::Fg(Color::Indexed(Chan::from_offset(v, 90 - 8))),
            100..=107 => SgrAttr::Bg(Color::Indexed(Chan::from_offset(v, 100 - 8))),
            0 => SgrAttr::Reset,
            1 => SgrAttr::Set(Attrs::BOLD),
            4 => SgrAttr::UnderlineStyle(Attrs::UNDERLINE),
            39 => SgrAttr::Fg(Color::Default),
            49 => SgrAttr::Bg(Color::Default),
            _ => return None,
        })
    }

    /// Consumes as many parameters as the attribute needs and reports how many.
    /// Returning the consumed count is what keeps `38;2;r;g;b` from being
    /// misread as five separate attributes.
    pub fn parse(params: Params<'_>) -> Option<(SgrAttr, ParamCount)> {
        let v = params.at(ParamIdx::FIRST).map_or(0, |x| x.0);
        // **The colour introducers first, because they are what reaches here.**
        // `fold_sgr` short-circuits the single-parameter form to `parse_one`,
        // so what falls through to this match is the multi-parameter shapes --
        // and `38;2;r;g;b` is **47% of `btop.raw`'s** `SGR`s. The arms below
        // compile to a chain of unsigned range compares (`b.hs`/`b.hi`/`b.lo`
        // dominate the disassembly, not a jump table), and 38/48/58 sat near
        // its end behind thirty-odd tests it can never match.
        // **`v` is discriminated once, not twice.** This test used to be
        // `matches!(v, 38 | 48 | 58)` and `parse_color` then re-derived which
        // of the three it was from the same `u16` -- two compare chains over
        // one value, on the arm that is 47% of `btop.raw`'s `SGR`s and, by
        // `probe-double-sgr-color`, about three fifths of its parse.
        //
        // `ColorSlot` is that decision made once and carried. What reaches the
        // colour arm is a three-variant enum rather than a number, so choosing
        // the constructor is a discriminant test instead of a `u16` compare
        // chain, and the arm can no longer be reached with a `v` that names no
        // colour.
        #[cfg(not(feature = "colour-in-match"))]
        if let Some(slot) = ColorSlot::of(v) {
            return SgrAttr::parse_color(params, slot);
        }
        Some(match v {
            #[cfg(feature = "colour-in-match")]
            38 | 48 | 58 => {
                let slot = ColorSlot::of(v).expect("38, 48 and 58 each name a colour");
                return SgrAttr::parse_color(params, slot);
            }
            0 => (SgrAttr::Reset, ParamCount(1)),
            // `SGR 10-19`. Recorded rather than dropped: a font is a rendition,
            // and it lives in the rendition word's spare bits -- the row that
            // called this out of scope said fonts needed cell bits, and they
            // do not.
            10..=19 => (SgrAttr::Font(FontId::from_sgr(v)?), ParamCount(1)),
            1 => (SgrAttr::Set(Attrs::BOLD), ParamCount(1)),
            2 => (SgrAttr::Set(Attrs::DIM), ParamCount(1)),
            3 => (SgrAttr::Set(Attrs::ITALIC), ParamCount(1)),
            // `4 : n` -- the underline *style*, `[[source: kitty-underlines]]`:
            // 0 none, 1 straight, 2 double, 3 curly, 4 dotted, 5 dashed. A
            // colon group of two rather than a lone `4`, which stays the
            // straight underline it always was. Exactly one style is in force,
            // so the arm replaces the whole family: `Underline(style)` is a
            // masked replace over `UNDERLINES`, and an unknown `n` is refused
            // (`None`) rather than read as some other style. The group is
            // consumed whole, however long an emitter made it.
            4 if params.group_len(ParamIdx::FIRST) > 1 => {
                let group = params.group_len(ParamIdx::FIRST);
                let style = match params.at(ParamIdx::SECOND).map(|x| x.0) {
                    Some(0) => Attrs::NONE,
                    Some(1) => Attrs::UNDERLINE,
                    Some(2) => Attrs::DOUBLE_UNDERLINE,
                    Some(3) => Attrs::CURLY_UNDERLINE,
                    Some(4) => Attrs::DOTTED_UNDERLINE,
                    Some(5) => Attrs::DASHED_UNDERLINE,
                    // A style this core does not know clears the family
                    // rather than aborting the walk: `None` here would stop
                    // every parameter after it from applying, which is a
                    // worse answer than no underline.
                    _ => Attrs::NONE,
                };
                (SgrAttr::UnderlineStyle(style), ParamCount(group))
            }
            // A bare `4` is `4:1`, `[[source: kitty-underlines]]`'s backward-
            // compatible spelling: it *replaces* the family, so a `4` after a
            // `4:3` is a straight underline and not a curly-and-straight one.
            4 => (SgrAttr::UnderlineStyle(Attrs::UNDERLINE), ParamCount(1)),
            // 5 slow blink, 6 rapid blink. `[[source: ecma48]]` § 8.3.117
            // distinguishes them by rate, "less then 150 per minute" and "150
            // per minute or more". Both set BLINK, which the app blinks at 75
            // a minute: right for 5 and a stated departure for 6
            // (`docs/vt-coverage.md`, the SGR 5/6 row). This said "a terminal
            // core has no timer", which expired when the app's display link
            // began blinking the bit; a second bit waits on a rapid rate, which
            // no text or decision fixes, and until then DECRQSS answers 5, the
            // rendition drawn.
            5 | 6 => (SgrAttr::Set(Attrs::BLINK), ParamCount(1)),
            7 => (SgrAttr::Set(Attrs::REVERSE), ParamCount(1)),
            8 => (SgrAttr::Set(Attrs::HIDDEN), ParamCount(1)),
            9 => (SgrAttr::Set(Attrs::STRIKE), ParamCount(1)),
            // `21` is the double style of the underline family, `4:2` by its
            // ECMA-48 name, so it **replaces** the family as `4` and `4:n`
            // do. `[[source: ecma48]]` § 8.3.117 gives "21 doubly underlined"
            // and "24 not underlined (neither singly nor doubly)" -- two
            // values of one aspect with one cancel -- and closes with "The
            // usable combinations of parameter values are determined by the
            // implementation", so one style in force conforms. It was a
            // plain `Set`, so `4:3` then `21` held curly and double while
            // `21` then `4` held single alone: the order decided whether the
            // family was exclusive. Practice differs and is not followed:
            // vte reads 21 as bold off (`[[source: alacritty-vte-sgr]]`), the
            // Linux console as a single underline (`[[source: linux-vt-sgr]]`),
            // xterm holds 4 and 21 as two bits (`[[source: xterm-sgr]]`).
            21 => (
                SgrAttr::UnderlineStyle(Attrs::DOUBLE_UNDERLINE),
                ParamCount(1),
            ),
            22 => (SgrAttr::Unset(Attrs::BOLD.with(Attrs::DIM)), ParamCount(1)),
            20 => (SgrAttr::Set(Attrs::FRAKTUR), ParamCount(1)),
            // `[[source: ecma48]]` § 8.3.117 names 23 "not italicized, not
            // fraktur" -- one parameter clearing two attributes, so clearing
            // only italic would leave fraktur set with no way to turn it off.
            23 => (
                SgrAttr::Unset(Attrs::ITALIC.with(Attrs::FRAKTUR)),
                ParamCount(1),
            ),
            // `24` is `4:0`: every underline style off, not only the two it
            // used to name.
            24 => (SgrAttr::Unset(Attrs::UNDERLINES), ParamCount(1)),
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
            _ => return None,
        })
    }

    /// The direct-colour introducers, `SGR 38`, `48` and `58`.
    ///
    /// **58 shares this with 38 and 48 rather than getting a parallel copy**:
    /// the colon/semicolon distinction below is the part that is easy to get
    /// wrong, and one copy of it cannot drift from another.
    fn parse_color(params: Params<'_>, slot: ColorSlot) -> Option<(SgrAttr, ParamCount)> {
        // **Prices the colour arm apart from the 46-arm dispatch above it.**
        //
        // `probe-double-sgr` prices `parse` whole and `probe-double-fold` the
        // fold, so between them the parse is 15.3% of btop and 11.8% of
        // asciiquarium -- and nothing said *which part*. The two payloads
        // disagree about which arm they take: btop is 47% truecolour and
        // asciiquarium's SGRs are almost all `ESC [ 4 0 m`, a single parameter
        // that never reaches here. So doubling this arm alone separates the two
        // hypotheses -- if asciiquarium is unmoved and btop is not, the cost is
        // the colour parse; if neither moves, it is the dispatch.
        //
        // Pure: it reads `params` and returns, touching no counter, so the
        // discarded call changes nothing an arm could be accused of moving.
        // **The scratch, priced on its own.** `probe-double-sgr-color` calls
        // `reborrow`, which the real path does not, and this project has been
        // caught once already reporting an additive probe's scaffolding as the
        // thing it was measuring -- `probe-csi-scratch` exists because a
        // `Csi::NEW` zeroed 128 bytes per sequence that the real walk never
        // paid. So this arm does the reborrow and nothing else, and the colour
        // arm's cost is the difference.
        #[cfg(feature = "probe-sgr-color-reborrow")]
        {
            let again = params.reborrow();
            core::hint::black_box(&again);
        }
        #[cfg(feature = "probe-double-sgr-color")]
        {
            // `Params` is not `Copy`, and a clone would price the clone. It
            // borrows, so rebuilding one from the same slice and mask is the
            // same object by value at no cost the real path does not pay.
            let again = Self::parse_color_once(params.reborrow(), slot);
            core::hint::black_box(&again);
        }
        Self::parse_color_once(params, slot)
    }

    fn parse_color_once(params: Params<'_>, slot: ColorSlot) -> Option<(SgrAttr, ParamCount)> {
        Some({
            let mk = |c| slot.attr(c);
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
        })
    }
}

/// Which colour `38`, `48` and `58` name.
///
/// **The point is that it is decided once.** `38`/`48`/`58` were tested to
/// enter the colour arm and then tested again inside it to pick a constructor,
/// which is two compare chains over one `u16` on the arm
/// `probe-double-sgr-color` prices at about three fifths of btop's parse. A
/// three-variant enum makes the second one a discriminant test, and makes the
/// arm unreachable with a value that names no colour -- the same reason
/// `Digit::of_ascii` is infallible, which is the one change that has ever paid
/// on this path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ColorSlot {
    Fg,
    Bg,
    Underline,
}

impl ColorSlot {
    #[inline(always)]
    fn of(v: u16) -> Option<ColorSlot> {
        match v {
            38 => Some(ColorSlot::Fg),
            48 => Some(ColorSlot::Bg),
            58 => Some(ColorSlot::Underline),
            _ => None,
        }
    }
    #[inline(always)]
    fn attr(self, c: Color) -> SgrAttr {
        match self {
            ColorSlot::Fg => SgrAttr::Fg(c),
            ColorSlot::Bg => SgrAttr::Bg(c),
            ColorSlot::Underline => SgrAttr::Underline(c),
        }
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
    /// Fold this attribute into `cur` **in place**.
    ///
    /// [`SgrAttr::fold`] copies the twenty-byte rendition in and out to change
    /// four bytes of it, and the caller then assigns the result back. SGR
    /// handling is roughly half of `asciiquarium.raw` by `opcost`, and one
    /// attribute was moving sixty bytes to set one field.
    ///
    /// **Idempotent, which is what lets it be priced additively.** `Reset`
    /// replaces, `Set`/`Unset` force bits, and the colour and font arms
    /// replace a field -- applying any of them twice lands on the same word,
    /// so `probe-double-fold` leaves every counter alone and separates the
    /// fold from the `parse` that feeds it. It is **0.8% of btop and 2.4% of
    /// asciiquarium**; parse and fold together are 16.9% and 10.8%, so the
    /// cost is the parse.
    #[inline(always)]
    pub fn fold_into(self, cur: &mut StyleWord) {
        #[cfg(feature = "probe-double-fold")]
        {
            let mut again = *cur;
            self.fold_once(&mut again);
            core::hint::black_box(&again);
        }
        self.fold_once(cur);
    }

    #[inline(always)]
    fn fold_once(self, cur: &mut StyleWord) {
        match self {
            SgrAttr::Reset => *cur = StyleWord::DEFAULT,
            SgrAttr::Set(a) => cur.set_attrs(cur.attrs().with(a)),
            SgrAttr::Unset(a) => cur.set_attrs(cur.attrs().without(a)),
            SgrAttr::UnderlineStyle(a) => {
                cur.set_attrs(cur.attrs().without(Attrs::UNDERLINES).with(a))
            }
            SgrAttr::Fg(c) => cur.set_fg(ColorWord::from_color(c)),
            SgrAttr::Bg(c) => cur.set_bg(ColorWord::from_color(c)),
            SgrAttr::Font(f) => cur.set_font(f),
            SgrAttr::Underline(c) => cur.set_underline(ColorWord::from_color(c)),
        }
    }

    /// Apply one attribute to the accumulating rendition.
    ///
    /// Every variant is handled. It used to fall through to `cur.bits()` for
    /// `Set`, `Unset` and every `Bg`, so bold, italic, underline, reverse,
    /// blink, conceal, strike and all background colours were parsed and then
    /// silently discarded.
    pub fn fold(self, cur: StyleWord) -> StyleWord {
        match self {
            // `[[source: ecma48]]` § 8.3.117: SGR 0 "cancels the effect of any
            // preceding occurrence of SGR in the data stream".
            SgrAttr::Reset => StyleWord::DEFAULT,
            SgrAttr::Set(a) => cur.with_attrs(cur.attrs().with(a)),
            SgrAttr::Unset(a) => cur.with_attrs(cur.attrs().without(a)),
            SgrAttr::UnderlineStyle(a) => {
                cur.with_attrs(cur.attrs().without(Attrs::UNDERLINES).with(a))
            }
            SgrAttr::Fg(c) => cur.with_fg(ColorWord::from_color(c)),
            SgrAttr::Bg(c) => cur.with_bg(ColorWord::from_color(c)),
            SgrAttr::Font(f) => cur.with_font(f),
            SgrAttr::Underline(c) => cur.with_underline(ColorWord::from_color(c)),
        }
    }
}

/// Every single-parameter `SGR` value, resolved at compile time.
///
/// **Rebuilt after a destructive edit truncated this file**, from `parse`'s
/// own one-parameter arms, and `the_one_parameter_table_agrees_with_the_grammar`
/// holds it to them for all 128 values -- so the table cannot mean something
/// the grammar does not, whatever the reconstruction got wrong.
static ONE: [Option<SgrAttr>; SgrAttr::ONE_MAX] = build_one();

const fn build_one() -> [Option<SgrAttr>; SgrAttr::ONE_MAX] {
    let mut t = [None; SgrAttr::ONE_MAX];
    let mut v = 0usize;
    while v < SgrAttr::ONE_MAX {
        t[v] = match v as u16 {
            0 => Some(SgrAttr::Reset),
            1 => Some(SgrAttr::Set(Attrs::BOLD)),
            2 => Some(SgrAttr::Set(Attrs::DIM)),
            3 => Some(SgrAttr::Set(Attrs::ITALIC)),
            4 => Some(SgrAttr::UnderlineStyle(Attrs::UNDERLINE)),
            5 | 6 => Some(SgrAttr::Set(Attrs::BLINK)),
            7 => Some(SgrAttr::Set(Attrs::REVERSE)),
            8 => Some(SgrAttr::Set(Attrs::HIDDEN)),
            9 => Some(SgrAttr::Set(Attrs::STRIKE)),
            20 => Some(SgrAttr::Set(Attrs::FRAKTUR)),
            21 => Some(SgrAttr::UnderlineStyle(Attrs::DOUBLE_UNDERLINE)),
            22 => Some(SgrAttr::Unset(Attrs::BOLD.with(Attrs::DIM))),
            23 => Some(SgrAttr::Unset(Attrs::ITALIC.with(Attrs::FRAKTUR))),
            24 => Some(SgrAttr::Unset(Attrs::UNDERLINES)),
            25 => Some(SgrAttr::Unset(Attrs::BLINK)),
            26 => Some(SgrAttr::Set(Attrs::PROPORTIONAL)),
            27 => Some(SgrAttr::Unset(Attrs::REVERSE)),
            28 => Some(SgrAttr::Unset(Attrs::HIDDEN)),
            29 => Some(SgrAttr::Unset(Attrs::STRIKE)),
            39 => Some(SgrAttr::Fg(Color::Default)),
            49 => Some(SgrAttr::Bg(Color::Default)),
            50 => Some(SgrAttr::Unset(Attrs::PROPORTIONAL)),
            51 => Some(SgrAttr::Set(Attrs::FRAMED)),
            52 => Some(SgrAttr::Set(Attrs::ENCIRCLED)),
            53 => Some(SgrAttr::Set(Attrs::OVERLINED)),
            54 => Some(SgrAttr::Unset(Attrs::FRAMED.with(Attrs::ENCIRCLED))),
            55 => Some(SgrAttr::Unset(Attrs::OVERLINED)),
            59 => Some(SgrAttr::Underline(Color::Default)),
            60 => Some(SgrAttr::Set(Attrs::IDEOGRAM_UNDERLINE)),
            61 => Some(SgrAttr::Set(Attrs::IDEOGRAM_DOUBLE_UNDERLINE)),
            62 => Some(SgrAttr::Set(Attrs::IDEOGRAM_OVERLINE)),
            63 => Some(SgrAttr::Set(Attrs::IDEOGRAM_DOUBLE_OVERLINE)),
            64 => Some(SgrAttr::Set(Attrs::IDEOGRAM_STRESS)),
            65 => Some(SgrAttr::Unset(Attrs::IDEOGRAM)),
            n @ 10..=19 => match FontId::from_sgr(n) {
                Some(f) => Some(SgrAttr::Font(f)),
                None => None,
            },
            n @ 30..=37 => Some(SgrAttr::Fg(Color::Indexed(Chan::from_offset(n, 30)))),
            n @ 40..=47 => Some(SgrAttr::Bg(Color::Indexed(Chan::from_offset(n, 40)))),
            n @ 90..=97 => Some(SgrAttr::Fg(Color::Indexed(Chan::from_offset(n, 90 - 8)))),
            n @ 100..=107 => Some(SgrAttr::Bg(Color::Indexed(Chan::from_offset(n, 100 - 8)))),
            _ => None,
        };
        v += 1;
    }
    t
}

#[cfg(test)]
mod one_table {
    use crate::*;

    /// **The table must mean exactly what the grammar means**, for every value
    /// it covers and every value it declines. It was rebuilt by hand after a
    /// destructive edit truncated this file, so this is the only thing
    /// standing between the reconstruction and a silently wrong rendition.
    #[test]
    fn the_one_parameter_table_agrees_with_the_grammar() {
        let mut covered = 0usize;
        for v in 0..SgrAttr::ONE_MAX {
            let p = [Some(Param::from_raw(v as u16))];
            let want = SgrAttr::parse(Params::new(&p));
            let got = SgrAttr::one(Param::from_raw(v as u16));
            match (want, got) {
                // The table holds only what one parameter fully determines.
                (Some((a, n)), Some(b)) => {
                    assert_eq!(n.0, 1, "SGR {v} needs more than one parameter");
                    assert!(a == b, "SGR {v}: table and grammar disagree");
                    covered += 1;
                }
                (None, None) => {}
                // `38`, `48`, `58` parse only with following parameters, so the
                // grammar declines a lone one and the table must too.
                (Some((_, n)), None) => assert!(
                    n.0 != 1 || matches!(v, 38 | 48 | 58),
                    "SGR {v}: the grammar resolves it and the table does not"
                ),
                (None, Some(_)) => panic!("SGR {v}: the table invents an attribute"),
            }
        }
        assert!(covered >= 60, "only {covered} values covered");
    }
}

#[cfg(test)]
mod double_underline {
    use crate::*;

    /// **`21` and `4:2` are one attribute, built by the grammar and by the
    /// table alike.** `[[source: ecma48]]` § 8.3.117's "21 doubly underlined"
    /// and `[[source: kitty-underlines]]`' "4:2 double underline" name the
    /// same rendition, and baud holds the underline as a family with exactly
    /// one style in force; an `SgrAttr` that sets the double bit without
    /// replacing the family is a second representation of the same state
    /// that disagrees with the first as soon as another style is held.
    #[test]
    fn twenty_one_parses_to_the_double_style_of_the_family() {
        let ecma = [Some(Param::from_raw(21))];
        let kitty = [Some(Param::from_raw(4)), Some(Param::from_raw(2))];
        let colon = SubParams::NONE.mark(ParamIdx::SECOND);
        let a = SgrAttr::parse(Params::new(&ecma)).map(|(a, _)| a);
        let b = SgrAttr::parse(Params::with_subs(&kitty, colon)).map(|(a, _)| a);
        let double = Some(SgrAttr::UnderlineStyle(Attrs::DOUBLE_UNDERLINE));
        assert!(a == double, "21 is the double style, replacing the family");
        assert!(b == double, "4:2 is the double style, replacing the family");
        assert!(
            SgrAttr::one(Param::from_raw(21)) == double,
            "and the one-parameter table says the same"
        );
    }
}
