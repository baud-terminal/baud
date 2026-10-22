//! **A face as discovery reads it**: its family names and the three things
//! a family's faces are told apart by -- width, slope and weight, the order
//! `[[source: css-fonts-3]]` 5.2 narrows a family by -- read from the
//! directory and five small tables, with nothing else of the face parsed.
//! Discovery reads every installed face this way and parses only the ones
//! it picks ([`Face::parse_index`](crate::Face::parse_index)).
//!
//! * **Family**: every nameID 16 and 1 record (`[[source: ms-opentype-name]]`),
//!   since a name typed in a config may be any of them
//!   ([`Described::answers_to`]).
//! * **Weight**: `OS/2.usWeightClass` (`[[source: ms-opentype-os2-2026-10-21]]`:
//!   "Values from 1 to 1000 are valid ... 400 Normal (Regular) ... 700
//!   Bold"), the scale CSS's `font-weight` uses and CSS's Appendix A names
//!   for it ("The weight of a given face can be determined via the
//!   usWeightClass field of the OS/2 table"). Not `fsSelection`'s BOLD bit:
//!   that bit marks the style-linked bold of a four-face group, and Apple SD
//!   Gothic Neo's Bold, at 700, leaves it clear. A face with no `OS/2` falls
//!   back to `head.macStyle` bit 0 (`[[source: ms-opentype-head]]`), 700 for
//!   bold and 400 else.
//! * **Width**: `usWidthClass` 1 to 9 as the OS/2 table's "% of normal"
//!   column gives them, 50 to 200; 100 without an `OS/2`.
//! * **Slope**: `fsSelection` bit 9 OBLIQUE ("Font contains oblique
//!   glyphs"), else bit 0 ITALIC ("Font contains italic or oblique glyphs");
//!   with no `OS/2`, `head.macStyle` bit 1. An oblique face's angle is
//!   `post.italicAngle` negated (counter-clockwise degrees in the table,
//!   a clockwise lean as CSS measures it), or CSS's 14 degrees where the
//!   table gives none.
//! * **Axes**: `fvar`'s, so a variable face covers the range its axes
//!   reach.

use crate::bytes::{Bytes, Offset};
use crate::face::Directory;
use crate::name::{families_in, family_in};
use crate::var::{Axis, Fixed, Fvar};
use crate::{FaceIndex, Family, Tag};

/// A weight on the `usWeightClass` and `wght` scale, 1 to 1000.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub struct Weight(f32);

impl Weight {
    pub const REGULAR: Weight = Weight(400.0);
    pub const BOLD: Weight = Weight(700.0);

    pub const fn of(v: f32) -> Weight {
        Weight(v)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A width as a percentage of normal, the `wdth` axis's scale.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub struct Width(f32);

impl Width {
    pub const NORMAL: Width = Width(100.0);

    pub const fn of(v: f32) -> Width {
        Width(v)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A slant in degrees, clockwise positive, as CSS's `oblique` measures it.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub struct Angle(f32);

impl Angle {
    /// CSS's angle for an `oblique` with none given.
    pub const OBLIQUE: Angle = Angle(14.0);

    pub const fn of(v: f32) -> Angle {
        Angle(v)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Whether a face's glyphs are upright, italic, or obliqued by an angle.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Slope {
    Upright,
    Italic,
    Oblique(Angle),
}

/// A face's names and style, from its directory and five tables.
#[derive(Clone, Copy)]
pub struct Described<'a> {
    dir: Directory<'a>,
}

impl<'a> Described<'a> {
    /// Face `index` of `data`, or `None` when `data` is not a font or has
    /// no such face. Nothing past the directory is checked here: a table
    /// that does not read is a value that falls back, and the face is only
    /// parsed whole once it is picked.
    pub fn of(data: &'a [u8], index: FaceIndex) -> Option<Described<'a>> {
        Directory::of(data, index).ok().map(|dir| Described { dir })
    }

    /// The family the module's order in `name.rs` prefers, as
    /// [`crate::Face::family`] gives it.
    pub fn family(&self) -> Option<Family<'a>> {
        family_in(self.dir.table(Tag::NAME)?)
    }

    /// Every family record, nameIDs 16 and 1, every language.
    pub fn families(&self) -> impl Iterator<Item = Family<'a>> {
        self.dir.table(Tag::NAME).into_iter().flat_map(families_in)
    }

    /// Whether `name` is one of its family names, by default caseless
    /// match ([`Family::matches`]).
    pub fn answers_to(&self, name: &[u8]) -> bool {
        self.families().any(|f| f.matches(name))
    }

    fn os2(&self) -> Option<Bytes<'a>> {
        self.dir.table(Tag::OS2)
    }

    fn mac_style(&self) -> Option<MacStyle> {
        let head = self.dir.table(Tag::HEAD)?;
        head.read::<u16>(Offset::of(44u32)).map(MacStyle)
    }

    /// `usWeightClass`, else `head.macStyle`'s bold bit as 700, else 400.
    pub fn weight(&self) -> Weight {
        if let Some(w) = self.os2().and_then(|t| t.read::<u16>(Offset::of(4u32))) {
            if (1..=1000).contains(&w) {
                return Weight(f32::from(w));
            }
        }
        match self.mac_style().is_some_and(MacStyle::bold) {
            true => Weight::BOLD,
            false => Weight::REGULAR,
        }
    }

    /// `usWidthClass` as a percentage of normal; 100 without one, or for a
    /// value outside 1 to 9.
    pub fn width(&self) -> Width {
        let class = self.os2().and_then(|t| t.read::<u16>(Offset::of(6u32)));
        Width(match class {
            Some(1) => 50.0,
            Some(2) => 62.5,
            Some(3) => 75.0,
            Some(4) => 87.5,
            Some(6) => 112.5,
            Some(7) => 125.0,
            Some(8) => 150.0,
            Some(9) => 200.0,
            _ => 100.0,
        })
    }

    /// `fsSelection` bit 9, else bit 0; `head.macStyle` bit 1 without an
    /// `OS/2` that reaches `fsSelection`.
    pub fn slope(&self) -> Slope {
        let selection = self.os2().and_then(|t| t.read::<u16>(Offset::of(62u32)));
        let (italic, oblique) = match selection {
            Some(s) => (s & 1 != 0, s & 0x200 != 0),
            None => (self.mac_style().is_some_and(MacStyle::italic), false),
        };
        match (oblique, italic) {
            (true, _) => Slope::Oblique(self.oblique_angle()),
            (false, true) => Slope::Italic,
            (false, false) => Slope::Upright,
        }
    }

    /// `post.italicAngle` negated, when it leans clockwise; else CSS's 14.
    fn oblique_angle(&self) -> Angle {
        let angle = self
            .dir
            .table(Tag::POST)
            .and_then(|t| t.read::<Fixed>(Offset::of(4u32)));
        match angle {
            Some(a) if a < Fixed::ZERO => Angle(-a.to_f32()),
            _ => Angle::OBLIQUE,
        }
    }

    /// The `fvar` axis `tag`, if the face is variable along it.
    pub fn axis(&self, tag: Tag) -> Option<Axis> {
        let fvar = Fvar::parse(self.dir.table(Tag::FVAR)?)?;
        let mut i = crate::bytes::Index::ZERO;
        while let Some(a) = fvar.axis(i) {
            if a.tag == tag {
                return Some(a);
            }
            i = i.next();
        }
        None
    }
}

/// `head.macStyle`.
#[derive(Clone, Copy)]
struct MacStyle(u16);

impl MacStyle {
    fn bold(self) -> bool {
        self.0 & 1 != 0
    }

    fn italic(self) -> bool {
        self.0 & 2 != 0
    }
}
