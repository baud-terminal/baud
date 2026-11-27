//! **iTerm2's badge**: "a large text label that appears in the top right
//! of a terminal session" (`[[source: iterm2-badges]]`), "behind its text"
//! (`[[source: iterm2-doc-profiles-general]]`), which a program sets with
//! `OSC 1337 ; SetBadgeFormat`.
//!
//! **Laid out as iTerm2 lays it out** (`iTermBadgeLabel`,
//! `[[source: iterm2-964a75d-badge-label]]`; `it_drawInRect`,
//! `[[source: iterm2-964a75d-nsstringiterm]]`): the text split at its
//! newlines, each part wrapped to a box a fraction of the view wide and
//! high, every line right-aligned, at the largest whole point size from 4
//! to 100 that fits, found by iTerm2's own binary search. Lines break
//! where Unicode's line breaking algorithm allows (`[[source: uax14]]`, as
//! CoreText's word wrapping does), a line that must break doing so, and a
//! word wider than the box at a grapheme cluster boundary, as word
//! wrapping does when a word "doesn't fit on a single line". The box sits
//! `right` from the view's right edge and `top` from its top, iTerm2's
//! indicator height and margin (`rectForBadgeImageOfSize`,
//! `[[source: iterm2-7ce037e-text-drawing-helper]]`). The text is laid on
//! the grid of the badge's own face (`Stack::badge`), baud's embedded
//! JetBrains Mono Bold whatever font the text is in, as iTerm2's badge has
//! a face of its own, scaled to the point size: a character takes the
//! columns it takes on the screen.

use crate::atlas::Tex;
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use crate::stack::FaceStyle;
use baud_domain::{Chan, Classified, ClusterBreak, Codepoint, Cols, LineBreak, Width, WidthPolicy};
use baud_font::Px;

/// How the badge is drawn while there is one.
#[derive(Clone, PartialEq, Debug)]
pub struct BadgeLook {
    /// The text, its format evaluated.
    pub text: String,
    /// Its colour, at the alpha it is drawn at.
    pub colour: Argb,
    /// **Its outline's colour**, at the alpha it is drawn at: iTerm2's
    /// background colour, opaque, inside the alpha the badge is drawn at
    /// ([`DRAWN_AT`]), a line [`STROKE`] of the point size wide drawn over
    /// each glyph; transparent for none.
    pub outline: Argb,
    /// **The alpha the badge is drawn at** ([`DRAWN_AT`] by default): a
    /// colour glyph's own alpha is drawn inside it, as iTerm2 draws the
    /// whole label "alpha:_fillColor.alphaComponent".
    pub drawn_at: Chan,
    /// The largest box, as fractions of the view's width and height.
    pub fraction: (f32, f32),
    /// The box's distance from the view's top and right edges, device
    /// pixels.
    pub top: Px,
    pub right: Px,
    /// Device pixels a point.
    pub scale: f32,
    /// The face style it is drawn in.
    pub style: FaceStyle,
}

/// **iTerm2's default badge colour**, calibrated RGB (1, 0, 0) at 0.5
/// (`KEY_BADGE_COLOR`, `[[source: iterm2-964a75d-profile-preferences]]`),
/// in sRGB through the calibrated space's conversion, drawn as iTerm2
/// draws it: inside a context at the colour's own alpha, 0.5 of 0.5, a
/// quarter, 64 of 255. Pinned by
/// `the_badge_colour_is_iterm2s_default_in_srgb`.
pub const BADGE: Argb = Argb(0x40FF_2600);

/// **The alpha iTerm2 draws its badge at**, the badge colour's own: half,
/// for its default colour, and whole for an opaque one, as `SetColors`
/// makes every colour it sets. Its fill is that colour inside it, so a
/// quarter by default ([`BADGE`]), and its outline the background colour
/// "with alpha component" one inside it (`iTermBadgeLabel`'s
/// `imageWithPointSize:`, `[[source: iterm2-964a75d-badge-label]]`).
pub const DRAWN_AT: Chan = Chan::from_offset(0x80, 0);

/// **The outline's width, as a share of the point size**: iTerm2 draws the
/// badge's text with `NSStrokeWidthAttributeName: @-2` (`[[source:
/// iterm2-964a75d-badge-label]]`), a stroke width "interpreted as a
/// percentage of font point size" where "negative values are for stroke
/// and fill" (`[[source: apple-ct-stroke-width]]`), in the colour
/// `NSStrokeColorAttributeName` names, the background's; the line
/// "straddles the path, with half of the total width on either side"
/// (`[[source: apple-quartz2d-paths]]`), drawn by `baud_font::Stroker`.
pub const STROKE: f32 = 0.02;

/// iTerm2's box, the largest the badge fits in, as fractions of the view's
/// width and height (`badgeMaxWidthFraction`, `badgeMaxHeightFraction`,
/// `[[source: iterm2-964a75d-advanced-settings]]`).
pub const BOX: (f32, f32) = (0.5, 0.2);

/// iTerm2's margins in points: the box's top 10 points below the 20-point
/// indicator strip (`kiTermIndicatorStandardHeight`, `[[source:
/// iterm2-964a75d-indicators-helper]]`), its right edge 10 points from the
/// view's.
pub const MARGINS: (f32, f32) = (30.0, 10.0);

/// **`inst` with the holes left out**, into `out`: what is left of its
/// rectangle once each hole is taken from it, in at most four pieces a
/// hole -- above, below, left and right of it -- each moving its texels with
/// it, since a badge record draws one texel a pixel.
pub(crate) fn around(inst: Instance, holes: &[[i32; 4]], out: &mut Vec<Instance>) {
    let mut pieces = vec![inst];
    for h in holes {
        let mut next = Vec::new();
        for p in pieces {
            let (x0, y0) = (i32::from(p.x.0), i32::from(p.y.0));
            let (x1, y1) = (x0 + i32::from(p.w.0), y0 + i32::from(p.h.0));
            if h[0] >= x1 || h[2] <= x0 || h[1] >= y1 || h[3] <= y0 {
                next.push(p);
                continue;
            }
            let (cy0, cy1) = (h[1].max(y0), h[3].min(y1));
            let mut part = |left: i32, top: i32, right: i32, bottom: i32| {
                if right <= left || bottom <= top {
                    return;
                }
                let dx = u16::try_from(left - x0).unwrap_or(0);
                let dy = u16::try_from(top - y0).unwrap_or(0);
                next.push(Instance {
                    x: Screen(i16::try_from(left).unwrap_or(i16::MAX)),
                    y: Screen(i16::try_from(top).unwrap_or(i16::MAX)),
                    w: Tex(u16::try_from(right - left).unwrap_or(0)),
                    h: Tex(u16::try_from(bottom - top).unwrap_or(0)),
                    u: Tex(p.u.0.saturating_add(dx)),
                    v: Tex(p.v.0.saturating_add(dy)),
                    ..p
                });
            };
            part(x0, y0, x1, cy0);
            part(x0, cy1, x1, y1);
            part(x0, cy0, h[0].max(x0), cy1);
            part(h[2].min(x1), cy0, x1, cy1);
        }
        pieces = next;
    }
    out.extend(pieces);
}

/// iTerm2's smallest and largest point sizes (`iTermBadgeLabel`'s
/// `_minimumPointSize` and `_maximumPointSize`).
pub const POINTS: (u32, u32) = (4, 100);

/// One grapheme cluster of the text, with the columns it takes.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Piece {
    pub cps: Vec<Codepoint>,
    pub cells: u32,
    /// What may happen before it.
    pub before: LineBreak,
    pub space: Space,
}

/// Whether a piece is white space, which hangs past a line's end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Space {
    Yes,
    No,
}

/// The text as pieces, each paragraph's line breaks read by UAX #14.
pub(crate) fn pieces(text: &str) -> Vec<Piece> {
    let chars: Vec<char> = text.chars().collect();
    let mut breaks = vec![LineBreak::Prohibited; chars.len() + 1];
    baud_domain::line_breaks(&chars, |i, b| breaks[i] = b);
    let mut out: Vec<Piece> = Vec::new();
    let mut cluster = ClusterBreak::new();
    for (i, &c) in chars.iter().enumerate() {
        let cp = Codepoint::from_char(c);
        let starts = cluster.is_break(cp) || out.is_empty();
        if !starts {
            if let Some(last) = out.last_mut() {
                last.cps.push(cp);
            }
            continue;
        }
        // A control -- the newline that ends a line, a tab -- takes no
        // column and draws nothing, as the text drawn takes none for it.
        let cells = match (
            c.is_control(),
            Classified::of(cp).width(WidthPolicy::DEFAULT),
        ) {
            (true, _) | (_, Width::ZERO) => 0,
            (_, Width::WIDE) => 2,
            _ => 1,
        };
        out.push(Piece {
            cps: vec![cp],
            cells,
            before: breaks[i],
            space: match c.is_whitespace() || c.is_control() {
                true => Space::Yes,
                false => Space::No,
            },
        });
    }
    out
}

/// A laid-out line: the pieces it holds, and its width in columns, the
/// spaces that hang at its end not counted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Line {
    pub from: usize,
    pub to: usize,
    pub cells: u32,
}

/// The pieces in lines of at most `width` columns: a line ends where a
/// break is mandatory, or at the last allowed break before the piece that
/// would pass `width`, or, with none on the line, before that piece.
pub(crate) fn wrap(pieces: &[Piece], width: Cols) -> Vec<Line> {
    let width = u32::try_from(width.index()).unwrap_or(u32::MAX);
    let mut lines = Vec::new();
    let (mut from, mut last) = (0usize, None::<usize>);
    let mut i = 0;
    let shown = |a: usize, b: usize| {
        let end = (a..b)
            .rev()
            .find(|&k| pieces[k].space == Space::No)
            .map_or(a, |k| k + 1);
        pieces[a..end].iter().map(|p| p.cells).sum::<u32>()
    };
    while i < pieces.len() {
        let p = &pieces[i];
        if i > from && p.before == LineBreak::Mandatory {
            lines.push(Line {
                from,
                to: i,
                cells: shown(from, i),
            });
            (from, last) = (i, None);
            continue;
        }
        if i > from && p.before == LineBreak::Allowed {
            last = Some(i);
        }
        if p.space == Space::No && shown(from, i + 1) > width && i > from {
            let at = last.unwrap_or(i);
            lines.push(Line {
                from,
                to: at,
                cells: shown(from, at),
            });
            (from, last) = (at, None);
            i = at;
            continue;
        }
        i += 1;
    }
    if from < pieces.len() || lines.is_empty() {
        lines.push(Line {
            from,
            to: pieces.len(),
            cells: shown(from, pieces.len()),
        });
    }
    lines
}

/// The face's grid at one size: a column's advance, the ascent to the
/// baseline and a line's height, unrounded, in device pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Grid {
    pub advance: f32,
    pub ascent: f32,
    pub height: f32,
}

/// **The point size iTerm2's search settles on** (`idealPointSize`):
/// between 4 and 100, halving, a size whose text passes the box lowering
/// the top and one that is inside it on both sides raising the bottom,
/// until the size stops moving; and the lines at it.
pub(crate) fn fit(
    pieces: &[Piece],
    grid: impl Fn(u32) -> Grid,
    max: (f32, f32),
) -> (u32, Vec<Line>, Grid) {
    let (mut min, mut maxp) = POINTS;
    let mut points = (min + maxp) / 2;
    let mut prev = None;
    let lay = |p: u32| {
        let g = grid(p);
        let columns = (max.0 / g.advance.max(0.01)).floor().clamp(0.0, 4096.0);
        let lines = wrap(
            pieces,
            Cols::of_index(usize::try_from(columns as i64).unwrap_or(0)),
        );
        let widest = lines.iter().map(|l| l.cells).max().unwrap_or(0);
        let w = f32::from(u16::try_from(widest).unwrap_or(u16::MAX)) * g.advance;
        let h = f32::from(u16::try_from(lines.len()).unwrap_or(u16::MAX)) * g.height;
        (lines, g, w, h)
    };
    while prev != Some(points) {
        let (_, _, w, h) = lay(points);
        if w > max.0 || h > max.1 {
            maxp = points;
        } else if w < max.0 && h < max.1 {
            min = points;
        }
        prev = Some(points);
        points = (min + maxp) / 2;
    }
    let (lines, g, _, _) = lay(points);
    (points, lines, g)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(text: &str, width: Cols) -> Vec<String> {
        let p = pieces(text);
        wrap(&p, width)
            .iter()
            .map(|l| {
                p[l.from..l.to]
                    .iter()
                    .flat_map(|q| q.cps.iter().filter_map(|c| char::from_u32(c.value())))
                    .collect()
            })
            .collect()
    }

    /// **Lines break where UAX #14 allows, as word wrapping does**: at the
    /// last break that fits, spaces hanging past the end; where it must,
    /// at a newline; and a word wider than the box at a cluster boundary.
    #[test]
    fn a_badge_wraps_as_word_wrapping_does() {
        assert_eq!(
            lines_of("one two three", Cols::of_index(7)),
            ["one two ", "three"]
        );
        assert_eq!(
            lines_of("one two three", Cols::of_index(30)),
            ["one two three"]
        );
        assert_eq!(lines_of("a\nb", Cols::of_index(30)), ["a\n", "b"]);
        assert_eq!(
            lines_of("abcdefghij", Cols::of_index(4)),
            ["abcd", "efgh", "ij"]
        );
        assert_eq!(lines_of("ab   cd", Cols::of_index(2)), ["ab   ", "cd"]);
        assert_eq!(
            lines_of("prod-db-01", Cols::of_index(6)),
            ["prod-", "db-01"]
        );
        let p = pieces("ab   cd");
        assert_eq!(
            wrap(&p, Cols::of_index(2))[0].cells,
            2,
            "the hanging spaces take no width"
        );
        assert_eq!(
            lines_of("日本語", Cols::of_index(4)),
            ["日本", "語"],
            "wide characters take two"
        );
    }

    /// **The point size iTerm2's search settles on fits, and one more does
    /// not**, on a grid whose column grows with the size: the halving
    /// search lands on the largest that fits.
    #[test]
    fn the_size_is_the_largest_that_fits() {
        let p = pieces("prod-db-01\nmain");
        let grid = |points: u32| {
            let px = f32::from(u16::try_from(points).unwrap_or(0));
            Grid {
                advance: px * 0.6,
                ascent: px,
                height: px * 1.2,
            }
        };
        let max = (400.0, 120.0);
        let (points, lines, _) = fit(&p, grid, max);
        let fits = |points: u32| {
            let g = grid(points);
            let columns = (max.0 / g.advance).floor();
            let l = wrap(
                &p,
                Cols::of_index(usize::try_from(columns as i64).unwrap_or(0)),
            );
            let w = l.iter().map(|x| x.cells).max().unwrap_or(0);
            f32::from(u16::try_from(w).unwrap_or(0)) * g.advance <= max.0
                && f32::from(u16::try_from(l.len()).unwrap_or(0)) * g.height <= max.1
        };
        assert!(fits(points), "{points} fits");
        assert!(!fits(points + 1), "{} does not", points + 1);
        assert_eq!(lines.len(), 2, "the newline's two lines");
    }
}
