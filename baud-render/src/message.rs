//! **baud's short messages: one pair of colours**, drawn the same on every
//! path, which a program cannot change.
//!
//! A field drawn in the selection's colours takes whatever a program last
//! set with `OSC 17` and `OSC 19`, reverse video's exchange under `?5`, and
//! the `OSC 10` and `OSC 11` defaults under it. A field whose hints name
//! [`crate::Hints::MESSAGE`] takes none of them: it is drawn in a rendition
//! built by hand ([`crate::Renderer::set_message_colours`]), through the one
//! function every path's fields go through, so the instance path, the CPU
//! path and the cell path through `Overlay::composed` draw the same pair.
//!
//! **What a colour names** (`[message] box_color` and `text_color`):
//!
//! * `"baud"`, [`Pair::BAUD`], on dark, on light and under every theme file;
//! * `"#rrggbb"`, that colour;
//! * `"none"`, the selection's colours as they are at draw.
//!
//! **The pair is measured against the colours in force, at draw**
//! ([`MessageColours::in_force`]): `"none"` reads what a program can set, so
//! under [`LEAST_CONTRAST`] the frame falls back to [`Pair::BAUD`]. The box
//! is opaque, so the pair is the whole measure: nothing behind it counts.
//! `[[source: wcag22]]`, SC 1.4.3, asks 4.5:1 of text this size. A pair of
//! two fixed colours under it is refused when the config is read, and said
//! ([`MessageColours::refused`]).
//!
//! Neither the shape nor the colours prove the box is baud's: a program can
//! paint the same cells. What it cannot do is change this one.

use crate::color::{Argb, contrast_of};
use baud_domain::Chan;

/// The least a message's text reads against its box: `[[source: wcag22]]`,
/// SC 1.4.3, Contrast (Minimum).
pub const LEAST_CONTRAST: f64 = 4.5;

/// **A box and the text on it.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pair {
    pub page: Argb,
    pub text: Argb,
}

impl Pair {
    /// **baud's own**: `#15171B` on `#F4A32C`, 8.65:1.
    pub const BAUD: Pair = Pair {
        page: Argb(0xFFF4_A32C),
        text: Argb(0xFF15_171B),
    };

    /// Whether it reads, by the measure above.
    pub fn reads(self) -> bool {
        contrast_of(self.text, self.page) >= LEAST_CONTRAST
    }
}

/// One colour of `[message]`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MessagePaint {
    /// `"baud"`: this half of [`Pair::BAUD`].
    #[default]
    Baud,
    /// `"#rrggbb"`.
    Colour(Argb),
    /// `"none"`: this half of the selection's colours, at draw.
    Selection,
}

/// `[message] box_color` and `text_color`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MessageColours {
    pub page: MessagePaint,
    pub text: MessagePaint,
}

/// Whether the selection is drawn in the message pair
/// ([`crate::Renderer::set_selection_pulse`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelectionPulse {
    #[default]
    Off,
    On,
}

fn solid(c: Argb) -> Argb {
    c.with_alpha(Chan::from_u8(0xFF))
}

impl MessageColours {
    /// **The pair a frame draws**, the selection being `selection` in the
    /// colours in force: what each names, or [`Pair::BAUD`] where the two
    /// do not read. Opaque whatever `selection` carried.
    pub fn in_force(self, selection: Pair) -> Pair {
        let pair = Pair {
            page: solid(match self.page {
                MessagePaint::Baud => Pair::BAUD.page,
                MessagePaint::Colour(c) => c,
                MessagePaint::Selection => selection.page,
            }),
            text: solid(match self.text {
                MessagePaint::Baud => Pair::BAUD.text,
                MessagePaint::Colour(c) => c,
                MessagePaint::Selection => selection.text,
            }),
        };
        match pair.reads() {
            true => pair,
            false => Pair::BAUD,
        }
    }

    /// **The two fixed colours when they do not read**, for the config's
    /// refusal; none while either is the selection's, which only a frame
    /// can measure.
    pub fn refused(self) -> Option<Pair> {
        let fixed = |p: MessagePaint, baud: Argb| match p {
            MessagePaint::Baud => Some(baud),
            MessagePaint::Colour(c) => Some(c),
            MessagePaint::Selection => None,
        };
        let pair = Pair {
            page: fixed(self.page, Pair::BAUD.page)?,
            text: fixed(self.text, Pair::BAUD.text)?,
        };
        (!pair.reads()).then_some(pair)
    }

    /// **Whether the pair has a colour of its own**: with both `"none"` it
    /// is the selection's, and a pulse in it would show nothing.
    pub fn has_its_own(self) -> bool {
        self.page != MessagePaint::Selection || self.text != MessagePaint::Selection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: MessageColours = MessageColours {
        page: MessagePaint::Selection,
        text: MessagePaint::Selection,
    };

    /// **baud's pair reads 8.65:1, as its doc comment says.**
    #[test]
    fn bauds_pair_reads() {
        let c = contrast_of(Pair::BAUD.text, Pair::BAUD.page);
        assert!((8.64..8.66).contains(&c), "{c}");
        assert!(Pair::BAUD.reads());
    }

    /// **A pair under 4.5:1 falls back to baud's at draw, and one over it
    /// stands**, whichever half a program moved; a fixed colour names
    /// itself; with both `"baud"` the selection has no part. Perturbation:
    /// drop the check in `in_force` and the grey pair draws.
    #[test]
    fn a_pair_is_measured_at_draw() {
        let grey = Pair {
            page: Argb(0xFF80_8080),
            text: Argb(0xFF90_9090),
        };
        assert_eq!(NONE.in_force(grey), Pair::BAUD, "grey on grey: baud's");
        let reads = Pair {
            page: Argb(0xFF00_0000),
            text: Argb(0xFFFF_FFFF),
        };
        assert_eq!(
            NONE.in_force(reads),
            reads,
            "the selection's, while it reads"
        );
        assert_eq!(
            MessageColours::default().in_force(grey),
            Pair::BAUD,
            "the selection has no part in baud's"
        );
        let mine = MessageColours {
            page: MessagePaint::Colour(Argb(0xFF00_0040)),
            text: MessagePaint::Colour(Argb(0xFFFF_FF00)),
        };
        assert_eq!(
            mine.in_force(grey),
            Pair {
                page: Argb(0xFF00_0040),
                text: Argb(0xFFFF_FF00)
            }
        );
    }

    /// **An opaque box whatever the selection carried**: a dimmed page
    /// through `"none"` is drawn solid.
    #[test]
    fn the_box_is_opaque() {
        let dimmed = Pair {
            page: Argb(0xFF00_0000).with_alpha(Chan::from_u8(0x40)),
            text: Argb(0xFFFF_FFFF),
        };
        assert_eq!(NONE.in_force(dimmed).page.a(), Chan::from_u8(0xFF));
    }

    /// **The config refuses two fixed colours that do not read, and leaves
    /// a `"none"` to the frame.**
    #[test]
    fn a_fixed_pair_is_refused_when_it_does_not_read() {
        let bad = MessageColours {
            page: MessagePaint::Colour(Argb(0xFF00_0000)),
            text: MessagePaint::Colour(Argb(0xFF11_1111)),
        };
        assert!(bad.refused().is_some());
        let against_bauds = MessageColours {
            page: MessagePaint::Colour(Argb(0xFF20_2020)),
            text: MessagePaint::Baud,
        };
        assert!(against_bauds.refused().is_some(), "against baud's own half");
        assert_eq!(MessageColours::default().refused(), None);
        let left_to_the_frame = MessageColours {
            page: MessagePaint::Selection,
            text: MessagePaint::Colour(Argb(0xFF00_0000)),
        };
        assert_eq!(left_to_the_frame.refused(), None);
    }

    /// **Both `"none"` has nothing of its own for a pulse to flash.**
    #[test]
    fn both_none_has_nothing_of_its_own() {
        assert!(MessageColours::default().has_its_own());
        assert!(!NONE.has_its_own());
        assert!(
            MessageColours {
                page: MessagePaint::Baud,
                ..NONE
            }
            .has_its_own()
        );
    }
}
