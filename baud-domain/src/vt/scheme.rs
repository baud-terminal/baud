//! The dark/light class of a pair of colours, for `CSI ? 996 n` and the
//! `?2031` notification.
//!
//! `[[source: contour-palette-notifications]]` specifies the query and the
//! report -- `CSI ? 997 ; 1 n` for dark, `CSI ? 997 ; 2 n` for light, and
//! nothing else -- and never says how a palette is classified. baud's rule is
//! note Y's decision 5: **WCAG relative luminance of the background against
//! the text**, dark when the background is the darker of the two. kitty and
//! iTerm2 classify the background alone by a luma under one half; the rules
//! agree on all 602 bundled ghostty themes and differ only near mid-grey,
//! where the pair is illegible either way (note Y, section 7).

use crate::ColorWord;

/// Dark or light, the only two answers the report has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorScheme {
    /// `CSI ? 997 ; 1 n`.
    Dark,
    /// `CSI ? 997 ; 2 n`.
    Light,
}

impl ColorScheme {
    /// The class of text in `fg` on a page in `bg`, or `None` when either is
    /// not an RGB colour -- the default or a palette index, which name no
    /// pixel on their own. **Dark when the background's relative luminance
    /// is below the foreground's**; a tie reads light.
    ///
    /// Relative luminance is WCAG 2.2's (`[[source: wcag22]]`): each channel
    /// linearised by the sRGB transfer function, then weighted 0.2126, 0.7152
    /// and 0.0722. The linearisation is [`LINEAR`], a table rather than a
    /// `powf` -- the core is `no_std` and has none -- and the weights are the
    /// same fractions in ten-thousandths, so the comparison is exact integer
    /// arithmetic over the table's values.
    pub fn of(fg: ColorWord, bg: ColorWord) -> Option<ColorScheme> {
        let fg = luminance(fg.as_rgb()?);
        let bg = luminance(bg.as_rgb()?);
        Some(if bg < fg {
            ColorScheme::Dark
        } else {
            ColorScheme::Light
        })
    }

    /// The report's parameter: 1 for dark, 2 for light.
    pub fn code(self) -> u16 {
        match self {
            ColorScheme::Dark => 1,
            ColorScheme::Light => 2,
        }
    }
}

/// Relative luminance in units of 1/(65535 * 10000): the three linear
/// channels from [`LINEAR`], weighted 2126, 7152 and 722. The largest value,
/// white, is 655,350,000, inside a `u32`.
fn luminance((r, g, b): (u8, u8, u8)) -> u32 {
    let l = |c: u8| u32::from(LINEAR[usize::from(c)]);
    2126 * l(r) + 7152 * l(g) + 722 * l(b)
}

/// The sRGB transfer function inverted, per channel value: `c / 12.92` up to
/// 0.04045 and `((c + 0.055) / 1.055)^2.4` above, both of `c / 255`, scaled to
/// 65535 and rounded. `baud-domain/tests/color_scheme.rs` recomputes every
/// entry in `f64` and requires this table to equal it.
pub const LINEAR: [u16; 256] = [
    0, 20, 40, 60, 80, 99, 119, 139, 159, 179, 199, 219, 241, 264, 288, 313, 340, 367, 396, 427,
    458, 491, 526, 562, 599, 637, 677, 718, 761, 805, 851, 898, 947, 997, 1048, 1101, 1156, 1212,
    1270, 1330, 1391, 1453, 1517, 1583, 1651, 1720, 1790, 1863, 1937, 2013, 2090, 2170, 2250, 2333,
    2418, 2504, 2592, 2681, 2773, 2866, 2961, 3058, 3157, 3258, 3360, 3464, 3570, 3678, 3788, 3900,
    4014, 4129, 4247, 4366, 4488, 4611, 4736, 4864, 4993, 5124, 5257, 5392, 5530, 5669, 5810, 5953,
    6099, 6246, 6395, 6547, 6700, 6856, 7014, 7174, 7335, 7500, 7666, 7834, 8004, 8177, 8352, 8528,
    8708, 8889, 9072, 9258, 9445, 9635, 9828, 10022, 10219, 10417, 10619, 10822, 11028, 11235,
    11446, 11658, 11873, 12090, 12309, 12530, 12754, 12980, 13209, 13440, 13673, 13909, 14146,
    14387, 14629, 14874, 15122, 15371, 15623, 15878, 16135, 16394, 16656, 16920, 17187, 17456,
    17727, 18001, 18277, 18556, 18837, 19121, 19407, 19696, 19987, 20281, 20577, 20876, 21177,
    21481, 21787, 22096, 22407, 22721, 23038, 23357, 23678, 24002, 24329, 24658, 24990, 25325,
    25662, 26001, 26344, 26688, 27036, 27386, 27739, 28094, 28452, 28813, 29176, 29542, 29911,
    30282, 30656, 31033, 31412, 31794, 32179, 32567, 32957, 33350, 33745, 34143, 34544, 34948,
    35355, 35764, 36176, 36591, 37008, 37429, 37852, 38278, 38706, 39138, 39572, 40009, 40449,
    40891, 41337, 41785, 42236, 42690, 43147, 43606, 44069, 44534, 45002, 45473, 45947, 46423,
    46903, 47385, 47871, 48359, 48850, 49344, 49841, 50341, 50844, 51349, 51858, 52369, 52884,
    53401, 53921, 54445, 54971, 55500, 56032, 56567, 57105, 57646, 58190, 58737, 59287, 59840,
    60396, 60955, 61517, 62082, 62650, 63221, 63795, 64372, 64952, 65535,
];
