//! **Where a link goes, before it is opened**: what a frame draws while
//! the link gesture's modifier is held over a link -- the link's cells
//! underlined and its address in a bar of its own -- for every path.
//!
//! The driver reads the link, its cells and the address it shows
//! (`baud-app`'s `link.rs`, which escapes it); this only draws them: the
//! underline as solids, the bar as a field drawn over its cells in the
//! message pair ([`crate::message`]), the find field's way ([`Field`]), so
//! nothing a program sets reaches the address.
//!
//! It also holds what is drawn with no modifier: a link's line at rest
//! ([`LinkLine`]) and the look of the link under the pointer ([`LinkHover`]).

use baud_domain::{Col, Row, StyleWord};

use crate::find::Field;

/// One row's run of a link's cells, both ends inclusive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkSpan {
    pub row: Row,
    pub from: Col,
    pub to: Col,
}

/// **What the link preview draws on the next frame**: the link's cells on
/// the view, a run a row, underlined, and the bar with its address.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkLook {
    pub spans: Vec<LinkSpan>,
    pub bar: Field,
}

/// **One run of a link's cells underlined at rest**, in one rendition: the
/// driver's scan (`baud-app`'s `lined.rs`) hands these over, and the line is
/// drawn in `[message]`'s box colour ([`LineColour::Box`]), baud's amber by
/// default, where the text this word resolves to draws; under the pointer,
/// and under [`LineColour::Text`], in that text's colour.
/// Nothing of it reaches a cell, so no query of the core's can read it back.
///
/// **A struck run is the same shape in a list of its own**
/// ([`crate::Renderer::set_link_strikes`]): struck through at the face's
/// strike position instead of underlined, and its text drawn in
/// [`MissingPaint`] where that reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkLine {
    pub row: Row,
    pub from: Col,
    pub to: Col,
    pub word: StyleWord,
}

/// `[link] line`: how a link is underlined at rest.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LineStyle {
    #[default]
    Solid,
    /// One thickness on, one off, as `SGR 4:4` draws.
    Dotted,
    /// `[link] line = "off"`: no line at rest. The scan still runs while
    /// `missing = "strike"`, for the struck runs, which are struck whatever
    /// the line is, and get no line under them.
    Off,
}

/// **What colour a link's line at rest takes.**
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LineColour {
    /// `[message]`'s box colour as each frame resolves it: baud's amber,
    /// `#F4A32C`, by default, on every theme.
    #[default]
    Box,
    /// The text's own colour, which `[link] hover = "line"` keeps at rest,
    /// since its look is a line in the box's colour.
    Text,
}

/// `[link] hover`: how the link under the pointer is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HoverPaint {
    /// Its cells in the message pair ([`crate::message`]), opaque.
    #[default]
    Box,
    /// A line in the pair's box colour; the text keeps its own.
    Line,
    /// Its text in the pair's box colour, on the cell's own ground, where
    /// that reads at least [`crate::linked::LINK_TEXT_CONTRAST`]; elsewhere
    /// the cell keeps its text colour, so a program cannot hide a link by
    /// painting that colour behind it.
    Text,
}

/// **`[link] missing_color`: what a struck link's text is drawn in**, a
/// struck link being one baud knows would open nothing -- its file gone on
/// this machine, or no app for its kind of link (`baud-app`'s `lined.rs`).
/// The strike is drawn whatever this says; the colour only where it reads
/// [`MISSING_CONTRAST`] on the cell, in the colours in force at draw, so a
/// program that moves colour 8 with `OSC 4` or paints the cell's ground
/// cannot make a struck link unreadable. Elsewhere the cell keeps its text
/// colour, and a selected cell keeps the selection's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MissingPaint {
    /// `"gray"`: palette colour 8 as it is at draw.
    #[default]
    Gray,
    /// `"#rrggbb"`.
    Colour(crate::color::Argb),
    /// `"none"`: the text keeps its own colour; only the strike shows.
    Kept,
}

/// The least a struck link's text reads on its cell for `missing_color`
/// to recolour it: `[[source: wcag22]]`, SC 1.4.3, Contrast (Minimum), the
/// floor text this size is asked to meet.
pub const MISSING_CONTRAST: f64 = 4.5;

/// The least the hovered text reads on its cell for `hover = "text"` to
/// recolour it. No standard names this floor: it is the one the design
/// chose so a light page still shows the look. The check on where a link
/// goes is the address in the bar, in its own pair.
pub const LINK_TEXT_CONTRAST: f64 = 1.5;

/// **The link under the pointer as the next frame draws it**: its cells, a
/// run a row, and how.
#[derive(PartialEq, Eq, Debug)]
pub struct LinkHover {
    pub spans: Vec<LinkSpan>,
    pub paint: HoverPaint,
}

impl Clone for LinkHover {
    fn clone(&self) -> LinkHover {
        LinkHover {
            spans: self.spans.clone(),
            paint: self.paint,
        }
    }

    /// In the buffer it has: a frame copies this over last frame's.
    fn clone_from(&mut self, source: &LinkHover) {
        self.spans.clone_from(&source.spans);
        self.paint = source.paint;
    }
}
