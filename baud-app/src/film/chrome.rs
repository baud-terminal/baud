//! **The window's title strip, composed over a frame drawn without a
//! window**: what the macOS app shows above the terminal's view -- AppKit's
//! traffic lights, baud's lockup beside them and the nine activity lights
//! at the strip's right -- laid over the README session's headless frames
//! so a still or a film of them reads as the window does. It is composed,
//! not filmed: nothing here is AppKit's drawing.
//!
//! * **Where**: the app's own placing -- the lockup by `mark::Place`
//!   beside the traffic lights' frames, the lights by `lights::placed` a
//!   light's gap past it, in a strip [`INSET`] points tall -- from the
//!   frames note AH §5.1's probe measured AppKit lay out in a titled
//!   window ([`BUTTONS`]), which the app reads off its window instead.
//! * **The lights**: each light's picture as the app draws it
//!   (`Renderer::activity_light` in baud's embedded JetBrains Mono Bold,
//!   the badge's face), in the look the app makes from the dark theme
//!   (`Renderer::lights_look`), on its plate where `lights::plate_frame`
//!   puts it, at the state the take's clocks showed at the frame
//!   (`session::shown_at`).
//! * **The lockup**: `assets/logo/baud-lockup-dark-tight.svg`, the form a
//!   dark window shows (`mark::Lockup`), its paths filled by baud-font's
//!   rasterizer.
//! * **The build's name**: after the lockup, where `mark::version_shows`
//!   allows it, in baud's embedded JetBrains Mono rather than the app's
//!   system font, at the same point size and in AppKit's secondary label
//!   colour ([`VERSION_INK`]): a stand-in too, for a label AppKit draws.
//! * **The rest is a stand-in**: the strip is one grey, the middle of the
//!   standard material's range under the dark appearance
//!   ([`GROUND_DARK`]), where the window's shows the desktop
//!   through it; the traffic lights are circles [`DOT`] points across in
//!   [`COLOURS`], measured off the filmed take.

use crate::lights::Spans;
use crate::mark::{Lockup, Place, VERSION_GAP, VERSION_POINTS, version_shows};
use baud_domain::Chan;
use baud_font::Px;
use baud_render::lights::{PLATE_RADIUS_TITLE, draw_plate};
use baud_render::{Argb, ByLed, ByLight, Glow, Led, Light, LightsImage, Renderer, Shown};

/// **The strip's grey**, the middle of the standard material's measured
/// range under the dark appearance, grey 33 to 92 (`Backdrop::
/// under_window_dark`): where the window shows the desktop through it.
pub(crate) const GROUND_DARK: Argb = Argb(0xFF_3A_3A_3A);
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{NSPoint, NSRect, NSSize};

/// **`NSColor.secondaryLabelColor` under Dark Aqua**, as AppKit gives it on
/// macOS 27: white at 55%. The composed name's ink; AppKit's own label
/// resolves it itself.
pub(crate) const VERSION_INK: Argb = Argb(0x8C_FF_FF_FF);

/// **The title strip's height, in points**: a titled window's on macOS
/// 27, whose `contentLayoutRect` is 28 points short of the window (note
/// AH §5.1); the app measures it off its window.
pub(crate) const INSET: CGFloat = 28.0;

/// **AppKit's traffic lights**, close, minimise and zoom: 14 by 16 point
/// frames at x 9, 29 and 49, y 6, in the strip (note AH §5.1; `mark.rs`'s
/// tests place the lockup beside the same frames).
pub(crate) const BUTTONS: [(CGFloat, CGFloat, CGFloat, CGFloat); 3] = [
    (9.0, 6.0, 14.0, 16.0),
    (29.0, 6.0, 14.0, 16.0),
    (49.0, 6.0, 14.0, 16.0),
];

/// **Each traffic light's circle, in points across**, centred in its
/// frame: the filmed take's, `assets/readme/hero.gif` as c6d97ceb holds
/// it, whose frame 60 shows each circle covering 17.7 pixels' worth of
/// area across at 1.477 pixels a point (its centres 29.5 pixels apart, 20
/// points), 12.0 points.
pub(crate) const DOT: CGFloat = 12.0;

/// **The traffic lights' colours in the dark appearance**, close, minimise
/// and zoom: each circle's middle in that `hero.gif`'s frame 60.
pub(crate) const COLOURS: [Argb; 3] = [
    Argb(0xFF_E0_59_5F),
    Argb(0xFF_EF_BE_4E),
    Argb(0xFF_68_D5_58),
];

/// **Where everything in the strip is**, in device pixels from the strip's
/// top left.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Layout {
    /// The strip's width and height.
    pub(crate) size: (Px, Px),
    /// Each traffic light's centre and radius.
    pub(crate) buttons: [(f64, f64, f64); 3],
    /// The lockup's left, top, width and height.
    pub(crate) lockup: (f64, f64, f64, f64),
    /// The build's name's left, in pixels; none when a narrow window has
    /// given it up.
    pub(crate) version: Option<Px>,
    /// Each activity light's left and width; the strip's height tall, from
    /// its top. None for a light a narrow window has no room for.
    pub(crate) lights: ByLed<Option<(Px, Px)>>,
    /// The lights' plate: its left, top, width and height, and the light
    /// it starts at; none with no light placed.
    pub(crate) plate: Option<(Px, Px, Px, Px)>,
}

/// The traffic lights' frames, as AppKit gives them.
fn buttons() -> [NSRect; 3] {
    BUTTONS.map(|(x, y, w, h)| NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)))
}

/// A length in pixels, rounded to the nearest whole one.
fn whole(v: f64) -> Px {
    Px::of(u32::try_from(v.round() as i64).unwrap_or(0))
}

/// A count of pixels as an index.
macro_rules! slots {
    ($v:expr) => {
        usize::try_from($v.value()).expect("a size")
    };
}

/// **The strip of a window `width` pixels wide at `scale`**, its lights
/// at `spans`: the lockup where `mark::Place::beside` puts it, the lights
/// where `lights::placed` does with the close button's gap at the right,
/// as the app keeps it, and the lockup's end -- or the end of the build's
/// name, `version_width` points across, while it fits -- as their left
/// bound.
pub(crate) fn layout(width: Px, scale: CGFloat, spans: &Spans, version_width: CGFloat) -> Layout {
    let [close, mini, zoom] = buttons();
    let art = Lockup::OnDark.art().expect("the lockup's size");
    let place = Place::beside(close, mini, zoom, INSET, art).expect("laid out");
    // Bottom-left origin, in a container the strip's height tall.
    let f = place.frame(INSET, INSET);
    let top = INSET - (f.origin.y + f.size.height);
    let pt_wide = CGFloat::from(width.value()) / scale;
    let shows = version_shows(
        pt_wide,
        close.origin.x,
        spans.width,
        place.end(),
        version_width,
    );
    let version_at = place.end() + VERSION_GAP;
    let left = match shows {
        true => version_at + version_width + VERSION_GAP,
        false => place.lights_left(),
    };
    let placed = crate::lights::placed(
        NSSize::new(pt_wide, INSET),
        INSET,
        close.origin.x,
        left,
        scale,
        spans,
    );
    let plate = crate::lights::plate_frame(&placed, INSET, scale).map(|(_, r)| {
        (
            whole(r.origin.x * scale),
            whole((INSET - r.origin.y - r.size.height) * scale),
            whole(r.size.width * scale),
            whole(r.size.height * scale),
        )
    });
    Layout {
        size: (width, whole(INSET * scale)),
        version: shows.then(|| whole(version_at * scale)),
        plate,
        buttons: [close, mini, zoom].map(|b| {
            (
                (b.origin.x + b.size.width / 2.0) * scale,
                (INSET - b.origin.y - b.size.height / 2.0) * scale,
                DOT / 2.0 * scale,
            )
        }),
        lockup: (
            f.origin.x * scale,
            top * scale,
            f.size.width * scale,
            f.size.height * scale,
        ),
        lights: ByLed::of(|led| {
            placed
                .get(led)
                .map(|(r, _)| (whole(r.origin.x * scale), whole(r.size.width * scale)))
        }),
    }
}

/// **The title strip, drawn once**: its pixels with every light dark
/// drawn in, and each light's three pictures, laid over it at each frame.
pub(crate) struct Strip {
    pub(crate) layout: Layout,
    /// The strip without its lights, premultiplied `0xAARRGGBB`, opaque,
    /// and with every light dark.
    ground: Vec<u32>,
    base: Vec<u32>,
    pictures: ByLed<ByLight<LightsImage>>,
}

impl Strip {
    /// **The strip of a window `width` pixels wide at `scale`**, `version`
    /// the build's name it says.
    pub(crate) fn new(width: Px, scale: CGFloat, version: &str) -> Strip {
        use baud_domain::{Cols, Rows};
        use baud_render::{Blend, PxSize, Stack, Theme};
        let faces = [baud_font::Face::parse(crate::embedded::BOLD.bytes).expect("the Bold")];
        let r = Renderer::new(
            Stack::new(&faces).expect("a stack"),
            PxSize::of(28.0).expect("a size"),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(6),
            Rows::of_index(2),
        )
        .expect("cell metrics");
        let spans = crate::lights::spans(&r, scale).expect("the lights' layout");
        let regular = baud_font::Face::parse(crate::embedded::REGULAR.bytes).expect("the face");
        let size = VERSION_POINTS as f32;
        let version_width = CGFloat::from(baud_render::lights::text_width(&regular, version, size));
        let layout = layout(width, scale, &spans, version_width);
        let look = r.lights_look(Glow::Full);
        let pictures = ByLed::of(|led| {
            ByLight::of(|light| {
                let mut img = LightsImage::new();
                r.activity_light(&look, led, light, scale as f32, INSET as f32, &mut img)
                    .expect("a light's picture");
                img
            })
        });
        let (w, h) = layout.size;
        let mut base = vec![GROUND_DARK.0; slots!(w) * slots!(h)];
        for ((cx, cy, radius), colour) in layout.buttons.iter().zip(COLOURS) {
            disc(&mut base, w, (*cx, *cy, *radius), colour);
        }
        lockup(&mut base, w, layout.lockup, Lockup::OnDark.svg());
        if let Some(x) = layout.version {
            let mut text = LightsImage::new();
            baud_render::lights::draw_text(
                &regular,
                version,
                size,
                VERSION_INK,
                scale as f32,
                INSET as f32,
                &mut text,
            )
            .expect("the name's picture");
            for row in 0..slots!(text.height) {
                for col in 0..slots!(text.width) {
                    let at = row * slots!(w) + slots!(x) + col;
                    if let (Some(d), Some(s)) = (
                        base.get_mut(at),
                        text.pixels.get(row * slots!(text.width) + col),
                    ) {
                        *d = over(Argb(*d), *s).0;
                    }
                }
            }
        }
        if let Some((x, y, pw, ph)) = layout.plate {
            let mut plate = LightsImage::new();
            let radius = PLATE_RADIUS_TITLE * scale as f32;
            draw_plate(&look, pw, ph, radius, &mut plate).expect("the plate");
            for row in 0..slots!(ph) {
                for col in 0..slots!(pw) {
                    let at = (slots!(y) + row) * slots!(w) + slots!(x) + col;
                    if let (Some(d), Some(s)) =
                        (base.get_mut(at), plate.pixels.get(row * slots!(pw) + col))
                    {
                        *d = over(Argb(*d), *s).0;
                    }
                }
            }
        }
        let mut strip = Strip {
            layout,
            ground: base.clone(),
            base,
            pictures,
        };
        let mut dark = strip.ground.clone();
        strip.lights(&mut dark, Shown::DARK);
        strip.base = dark;
        strip
    }

    /// The strip's height, in pixels.
    pub(crate) fn height(&self) -> Px {
        self.layout.size.1
    }

    /// Each light at its state in `shown`, over the strip's rows `px`.
    fn lights(&self, px: &mut [u32], shown: Shown) {
        let w = slots!(self.layout.size.0);
        for led in Led::ALL {
            let Some((x, _)) = *self.layout.lights.get(led) else {
                continue;
            };
            let img = self.pictures.get(led).get(*shown.get(led));
            let (iw, ih) = (slots!(img.width), slots!(img.height));
            for y in 0..ih.min(slots!(self.height())) {
                for i in 0..iw {
                    let at = y * w + slots!(x) + i;
                    if let (Some(d), Some(s)) = (px.get_mut(at), img.pixels.get(y * iw + i)) {
                        *d = over(Argb(*d), *s).0;
                    }
                }
            }
        }
    }

    /// **The window's frame**: the strip, its lights at `shown`, above
    /// `frame`, a frame of the strip's width, into `out`.
    pub(crate) fn compose(&self, shown: Shown, frame: &[u32], out: &mut Vec<u32>) {
        out.clear();
        // Every light dark is the strip as built; else each light is drawn
        // at its state over the strip without them.
        if Led::ALL.iter().all(|&l| *shown.get(l) == Light::Dark) {
            out.extend_from_slice(&self.base);
        } else {
            out.extend_from_slice(&self.ground);
            self.lights(out, shown);
        }
        out.extend_from_slice(frame);
    }
}

/// Straight-alpha `src` over the opaque `dst`.
fn over(dst: Argb, src: Argb) -> Argb {
    let (d, s) = (dst.0.to_be_bytes(), src.0.to_be_bytes());
    let a = u32::from(s[0]);
    let mix = |i: usize| {
        let v = (u32::from(s[i]) * a + u32::from(d[i]) * (255 - a) + 127) / 255;
        u8::try_from(v).unwrap_or(u8::MAX)
    };
    Argb(u32::from_be_bytes([0xff, mix(1), mix(2), mix(3)]))
}

/// `colour` at `coverage` over the opaque pixel `dst`.
fn cover(dst: Argb, colour: Argb, coverage: Chan) -> Argb {
    over(dst, colour.with_alpha(coverage))
}

/// A filled circle at `(cx, cy)` of `r`, its edge's pixels covered by how
/// far the edge is past their centres.
fn disc(px: &mut [u32], w: Px, (cx, cy, r): (f64, f64, f64), colour: Argb) {
    let w = slots!(w);
    for (i, p) in px.iter_mut().enumerate() {
        let (x, y) = ((i % w) as f64, (i / w) as f64);
        let (dx, dy) = (x + 0.5 - cx, y + 0.5 - cy);
        let c = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
        if c > 0.0 {
            *p = cover(Argb(*p), colour, Chan::from_unit(c as f32)).0;
        }
    }
}

/// One shape of the lockup's SVG: its fill and its outline's segments, in
/// the SVG's units.
#[derive(Debug, PartialEq)]
pub(crate) struct Shape {
    pub(crate) fill: Argb,
    pub(crate) segments: Vec<Segment>,
}

/// An SVG's view box: its left, top, width and height, in its units.
pub(crate) type ViewBox = (f32, f32, f32, f32);

/// A segment of an outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Segment {
    Line((f32, f32), (f32, f32)),
    Quad((f32, f32), (f32, f32), (f32, f32)),
}

/// **The lockup SVG's shapes and its view box**, as
/// `tools/baud-pictures` writes them: `<path>`s of absolute
/// `M`, `L`, `H`, `V`, `Q` and `Z` and `<rect>`s, each with a `#RRGGBB`
/// fill. Anything else is refused, so a picture the generator draws
/// otherwise fails here rather than drawing wrong.
pub(crate) fn shapes(svg: &[u8]) -> Result<(ViewBox, Vec<Shape>), String> {
    let text = std::str::from_utf8(svg).map_err(|e| e.to_string())?;
    let attr = |tag: &str, name: &str| -> Option<String> {
        Some(
            tag.split(&format!(" {name}=\""))
                .nth(1)?
                .split('"')
                .next()?
                .to_string(),
        )
    };
    let num = |s: &str| s.parse::<f32>().map_err(|e| format!("{s:?}: {e}"));
    let root = text.split("<svg").nth(1).ok_or("no <svg>")?;
    let root = root.split('>').next().ok_or("no <svg>")?;
    let vb: Vec<f32> = attr(root, "viewBox")
        .ok_or("no viewBox")?
        .split_whitespace()
        .map(num)
        .collect::<Result<_, _>>()?;
    let [x, y, w, h] = vb[..] else {
        return Err("a viewBox of four numbers".to_string());
    };
    let fill = |tag: &str| -> Result<Argb, String> {
        let f = attr(tag, "fill").ok_or("a shape with no fill")?;
        let hex = f
            .strip_prefix('#')
            .filter(|h| h.len() == 6)
            .ok_or(f.clone())?;
        let rgb = u32::from_str_radix(hex, 16).map_err(|e| format!("{f}: {e}"))?;
        Ok(Argb(0xff00_0000 | rgb))
    };
    let mut out = Vec::new();
    for piece in text.split('<').skip(1) {
        let tag = piece.split('>').next().unwrap_or("");
        let name = tag.split_whitespace().next().unwrap_or("");
        match name {
            "path" => out.push(Shape {
                fill: fill(tag)?,
                segments: path(&attr(tag, "d").ok_or("a path with no d")?)?,
            }),
            "rect" => {
                let n = |k: &str| num(&attr(tag, k).ok_or(format!("a rect with no {k}"))?);
                let (rx, ry, rw, rh) = (n("x")?, n("y")?, n("width")?, n("height")?);
                let c = [(rx, ry), (rx + rw, ry), (rx + rw, ry + rh), (rx, ry + rh)];
                out.push(Shape {
                    fill: fill(tag)?,
                    segments: (0..4)
                        .map(|i| Segment::Line(c[i], c[(i + 1) % 4]))
                        .collect(),
                });
            }
            "svg" | "/svg" | "!--" => {}
            _ if name.starts_with("!--") => {}
            other => {
                return Err(format!(
                    "an SVG element the lockup does not draw: <{other}>"
                ));
            }
        }
    }
    Ok(((x, y, w, h), out))
}

/// **An outline's segments** from a path's `d` of absolute `M`, `L`, `H`,
/// `V`, `Q` and `Z`; any other command is refused.
fn path(d: &str) -> Result<Vec<Segment>, String> {
    // Commands and numbers, a number ending where the next sign or
    // command starts.
    let mut tokens: Vec<String> = Vec::new();
    for c in d.chars() {
        match c {
            c if c.is_ascii_alphabetic() => tokens.push(c.to_string()),
            '-' => tokens.push("-".to_string()),
            ' ' | ',' | '\n' | '\t' => tokens.push(String::new()),
            c if c.is_ascii_digit() || c == '.' => match tokens.last_mut() {
                Some(t) if !t.chars().any(|c| c.is_ascii_alphabetic()) => t.push(c),
                _ => tokens.push(c.to_string()),
            },
            c => return Err(format!("{c:?} in a path")),
        }
    }
    tokens.retain(|t| !t.is_empty());
    let mut segs = Vec::new();
    let (mut at, mut start) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
    let mut i = 0;
    let mut cmd = ' ';
    let next = |i: &mut usize| -> Result<f32, String> {
        let t = tokens.get(*i).ok_or("a path ends mid-command")?;
        *i += 1;
        t.parse::<f32>().map_err(|e| format!("{t:?}: {e}"))
    };
    while i < tokens.len() {
        if let Some(c) = tokens[i].chars().next().filter(char::is_ascii_alphabetic) {
            cmd = c;
            i += 1;
            if cmd == 'Z' {
                if at != start {
                    segs.push(Segment::Line(at, start));
                }
                at = start;
                continue;
            }
        }
        match cmd {
            'M' => {
                at = (next(&mut i)?, next(&mut i)?);
                start = at;
                // Pairs after a moveto are lines.
                cmd = 'L';
            }
            'L' => {
                let to = (next(&mut i)?, next(&mut i)?);
                segs.push(Segment::Line(at, to));
                at = to;
            }
            'H' => {
                let to = (next(&mut i)?, at.1);
                segs.push(Segment::Line(at, to));
                at = to;
            }
            'V' => {
                let to = (at.0, next(&mut i)?);
                segs.push(Segment::Line(at, to));
                at = to;
            }
            'Q' => {
                let c = (next(&mut i)?, next(&mut i)?);
                let to = (next(&mut i)?, next(&mut i)?);
                segs.push(Segment::Quad(at, c, to));
                at = to;
            }
            other => {
                return Err(format!(
                    "the path command {other:?}, which the lockup does not use"
                ));
            }
        }
    }
    Ok(segs)
}

/// **The lockup at `(x, y, w, h)` pixels** over the strip's pixels, each
/// shape filled by baud-font's rasterizer in its own colour.
fn lockup(px: &mut [u32], w: Px, (x, y, bw, bh): (f64, f64, f64, f64), svg: &[u8]) {
    use baud_font::{Canvas, Extent, Point};
    let ((vx, vy, vw, vh), shapes) = shapes(svg).expect("the lockup's SVG");
    let h = Px::of(u32::try_from(px.len() / slots!(w)).expect("a height"));
    let (kx, ky) = ((bw / f64::from(vw)) as f32, (bh / f64::from(vh)) as f32);
    let to = |(u, v): (f32, f32)| Point::new((u - vx) * kx + x as f32, (v - vy) * ky + y as f32);
    let extent = Extent::new(w, h);
    let mut scratch = vec![0f32; extent.scratch().expect("a strip's cells")];
    let mut mask = vec![0u8; extent.cells().expect("a strip's cells")];
    for s in shapes {
        let mut c = Canvas::new(extent, &mut scratch).expect("the scratch");
        for seg in &s.segments {
            match *seg {
                Segment::Line(a, b) => c.line(to(a), to(b)),
                Segment::Quad(a, b, e) => c.quad(to(a), to(b), to(e)),
            }
        }
        c.coverage(&mut mask).expect("the mask");
        for (p, &m) in px.iter_mut().zip(&mask) {
            if m > 0 {
                *p = cover(Argb(*p), s.fill, Chan::from_u8(m)).0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The README film's window: 80 columns of the film's 17-pixel cell
    /// and the two 16-pixel paddings.
    const WIDE: Px = Px::of(1392);

    /// The build's name in these tests, whatever this build is called, so
    /// the layout pinned below is not a commit's.
    const VERSION: &str = "1.0.0-beta+0123abc.dirty";

    /// **The build's name follows the lockup in the strip, and gives way
    /// first**: in the film's window it starts a light's gap past the
    /// lockup's end, is inked there and every light stays; in a window too
    /// narrow for it and all nine it is gone and every light stays, as the
    /// app does it; narrower still the leftmost lights drop as they always
    /// did. Perturbation: place the lights against the lockup with the name
    /// shown and they overlap it, and the first half fails; show the name
    /// whatever the width and the second does.
    #[test]
    fn the_name_of_the_build_yields_before_the_lights() {
        let wide = Strip::new(WIDE, 2.0, VERSION);
        let l = &wide.layout;
        let (lx, _, lw, _) = l.lockup;
        let at = l.version.expect("the film's window has room");
        assert!(
            (f64::from(at.value()) - (lx + lw + VERSION_GAP * 2.0)).abs() <= 0.5,
            "a light's gap past the lockup, to the pixel: {at:?}"
        );
        let row = slots!(l.size.1) / 2;
        let ink = (slots!(at)..slots!(at) + 80).any(|x| {
            wide.base[(row - 2) * slots!(l.size.0) + x] != GROUND_DARK.0
                || wide.base[(row + 2) * slots!(l.size.0) + x] != GROUND_DARK.0
        });
        assert!(ink, "the name is drawn where the layout puts it");
        assert!(Led::ALL.into_iter().all(|led| l.lights.get(led).is_some()));

        let regular = baud_font::Face::parse(crate::embedded::REGULAR.bytes).unwrap();
        let across = CGFloat::from(baud_render::lights::text_width(
            &regular,
            VERSION,
            VERSION_POINTS as f32,
        ));
        let spans_of = |scale: CGFloat| {
            let faces = [baud_font::Face::parse(crate::embedded::BOLD.bytes).unwrap()];
            let r = Renderer::new(
                baud_render::Stack::new(&faces).unwrap(),
                baud_render::PxSize::of(28.0).unwrap(),
                baud_render::Theme::dark(),
                baud_render::Blend::Native,
                baud_domain::Cols::of_index(6),
                baud_domain::Rows::of_index(2),
            )
            .unwrap();
            crate::lights::spans(&r, scale).unwrap()
        };
        let spans = spans_of(2.0);
        let art = Lockup::OnDark.art().unwrap();
        let [c, m, z] = buttons();
        let end = Place::beside(c, m, z, INSET, art).unwrap().end();
        // All nine with the name, just; then one point short of that.
        let room = end + VERSION_GAP + across + VERSION_GAP + spans.width + c.origin.x;
        let px = |pt: f64| Px::of(u32::try_from((pt * 2.0).ceil() as i64).unwrap());
        let fits = layout(px(room), 2.0, &spans, across);
        assert!(fits.version.is_some(), "at {room} points");
        assert!(
            Led::ALL
                .into_iter()
                .all(|led| fits.lights.get(led).is_some())
        );
        let short = layout(px(room - 1.0), 2.0, &spans, across);
        assert_eq!(short.version, None, "a point short");
        assert!(
            Led::ALL
                .into_iter()
                .all(|led| short.lights.get(led).is_some()),
            "and no light gave way for it"
        );
        let narrow = layout(px(120.0), 2.0, &spans, across);
        assert_eq!(narrow.version, None);
        assert!(
            Led::ALL
                .into_iter()
                .any(|led| narrow.lights.get(led).is_none()),
            "narrower still, the lights drop as they did"
        );
    }

    /// **The strip is the app's, at the app's places**: 28 points tall,
    /// 56 pixels at 2x; the traffic lights centred in AppKit's frames, 16,
    /// 36 and 56 points along and 14 down; the lockup where
    /// `mark::Place::beside` puts it beside them, 69 points along, centred
    /// on the strip; all nine lights at the places `lights::placed` gives
    /// them in a window this wide, left to right in the panel's order, the
    /// last the close button's 9 points in from the right, the first a
    /// light's gap clear of the lockup. Perturbation: centre the lockup on
    /// the container's top edge rather than the strip's middle, or give
    /// `placed` no right gap, and this goes red.
    #[test]
    fn the_strip_has_the_apps_height_and_places() {
        let s = Strip::new(WIDE, 2.0, VERSION);
        let l = &s.layout;
        assert_eq!(l.size, (WIDE, Px::of(56)));
        assert_eq!(s.height(), Px::of(56));
        let centres: Vec<(f64, f64)> = l.buttons.iter().map(|b| (b.0, b.1)).collect();
        assert_eq!(centres, [(32.0, 28.0), (72.0, 28.0), (112.0, 28.0)]);
        assert!(l.buttons.iter().all(|b| b.2 == DOT / 2.0 * 2.0));
        let [c, m, z] = buttons();
        let art = Lockup::OnDark.art().unwrap();
        let place = Place::beside(c, m, z, INSET, art).unwrap();
        assert_eq!(place.x, 69.0);
        let (lx, ly, lw, lh) = l.lockup;
        assert_eq!(lx, 138.0);
        assert!((lw - place.width * 2.0).abs() < 1e-9 && (lh - place.height * 2.0).abs() < 1e-9);
        assert!(
            (ly + lh / 2.0 - 28.0).abs() < 1e-9,
            "centred on the strip: {ly} {lh}"
        );
        let name = crate::mark::VERSION_GAP * 2.0
            + CGFloat::from(baud_render::lights::text_width(
                &baud_font::Face::parse(crate::embedded::REGULAR.bytes).unwrap(),
                VERSION,
                crate::mark::VERSION_POINTS as f32,
            ));
        let left = (place.lights_left() + name) * 2.0;
        let mut last = left;
        for led in Led::ALL {
            let (x, w) = l.lights.get(led).expect("all nine in this window");
            let x = f64::from(x.value());
            assert!(x >= last, "{led:?} at {x}, left of {last}");
            last = x + f64::from(w.value());
        }
        assert_eq!(
            last,
            f64::from(WIDE.value() - 18),
            "the last light 9 points in"
        );
        let faces = [baud_font::Face::parse(crate::embedded::BOLD.bytes).unwrap()];
        let r = Renderer::new(
            baud_render::Stack::new(&faces).unwrap(),
            baud_render::PxSize::of(28.0).unwrap(),
            baud_render::Theme::dark(),
            baud_render::Blend::Native,
            baud_domain::Cols::of_index(6),
            baud_domain::Rows::of_index(2),
        )
        .unwrap();
        let spans = crate::lights::spans(&r, 2.0).unwrap();
        let placed = crate::lights::placed(
            NSSize::new(f64::from(WIDE.value()) / 2.0, INSET),
            INSET,
            9.0,
            place.lights_left(),
            2.0,
            &spans,
        );
        for led in Led::ALL {
            let (rect, _) = placed.get(led).unwrap();
            assert_eq!(
                *l.lights.get(led),
                Some((whole(rect.origin.x * 2.0), whole(rect.size.width * 2.0))),
                "{led:?}"
            );
            assert_eq!(rect.origin.y, 0.0, "{led:?} the strip's whole height");
        }
    }

    /// **The lights drawn in the strip follow the states given**: RD lit
    /// changes pixels inside RD's place and nowhere else, its lit picture
    /// over the plate there, and all dark is the strip as built.
    /// The frame below the strip is the frame given. The control: two
    /// states that differ only in RD differ only in RD's columns.
    /// Perturbation: compose every frame at `Shown::DARK` and RD lit
    /// matches dark.
    #[test]
    fn the_lights_in_the_strip_follow_the_states() {
        let s = Strip::new(WIDE, 2.0, VERSION);
        let below = vec![0xff12_3456u32; slots!(WIDE) * 3];
        let (mut dark, mut rd) = (Vec::new(), Vec::new());
        s.compose(Shown::DARK, &below, &mut dark);
        let lit = ByLed {
            rd: Light::Lit,
            ..Shown::DARK
        };
        s.compose(lit, &below, &mut rd);
        let strip = slots!(WIDE) * 56;
        assert_eq!(dark.len(), strip + below.len());
        assert_eq!(&dark[strip..], &below[..], "the frame under the strip");
        assert_eq!(&dark[..strip], &s.base[..]);
        let (x, w) = s.layout.lights.rd.unwrap();
        let mut changed = 0;
        for (i, (a, b)) in dark.iter().zip(&rd).enumerate() {
            if a != b {
                let col = u32::try_from(i % slots!(WIDE)).unwrap();
                let (x, w) = (x.value(), w.value());
                assert!(
                    i < strip && (x..x + w).contains(&col),
                    "a change outside RD at {i}"
                );
                changed += 1;
            }
        }
        assert!(changed > 20, "RD lit drew {changed} pixels");
        // RD's lit picture over the plate, along its middle row; the plate
        // is the dark theme's background, opaque.
        let img = s.pictures.rd.get(Light::Lit);
        let iw = slots!(img.width);
        let row = 28 * slots!(WIDE) + slots!(x);
        let bg = baud_render::Theme::dark()
            .bg
            .with_alpha(Chan::from_u8(0xFF));
        assert_eq!(s.ground[row - 2], bg.0, "the plate left of RD");
        for i in 0..iw {
            let want = over(Argb(s.ground[row + i]), img.pixels[28 * iw + i]).0;
            assert_eq!(rd[row + i], want, "column {i}");
        }
    }

    /// **The lockup's SVG reads as the generator writes it**: three shapes,
    /// the word in the dark lockup's light type and the mark and its block
    /// in its amber, inside the view box; and any other path command is
    /// refused rather than drawn wrong. The positive control: a path of
    /// every command it reads parses.
    #[test]
    fn the_lockups_svg_is_read_and_anything_else_refused() {
        let ((x, y, w, h), s) = shapes(Lockup::OnDark.svg()).expect("the lockup");
        assert_eq!(s.len(), 3);
        let fills: Vec<u32> = s.iter().map(|s| s.fill.0).collect();
        assert_eq!(fills, [0xFFF3_F4F6, 0xFFF4_A32C, 0xFFF4_A32C]);
        for seg in s.iter().flat_map(|s| &s.segments) {
            let pts = match *seg {
                Segment::Line(a, b) => vec![a, b],
                Segment::Quad(a, b, c) => vec![a, b, c],
            };
            for (u, v) in pts {
                assert!(u >= x - 0.01 && u <= x + w + 0.01, "{u}");
                assert!(v >= y - 0.01 && v <= y + h + 0.01, "{v}");
            }
        }
        let ok = path("M1 2L3 4H5V6Q7 8 9 10Z").unwrap();
        assert_eq!(ok.len(), 5);
        assert_eq!(ok[3], Segment::Quad((5.0, 6.0), (7.0, 8.0), (9.0, 10.0)));
        assert_eq!(ok[4], Segment::Line((9.0, 10.0), (1.0, 2.0)));
        assert_eq!(
            path("M1 2-3-4").unwrap(),
            [Segment::Line((1.0, 2.0), (-3.0, -4.0))]
        );
        for bad in ["M1 2C3 4 5 6 7 8", "m1 2l3 4", "M1 2A1 1 0 0 1 3 4"] {
            assert!(path(bad).is_err(), "{bad}");
        }
        let circle = br##"<svg width="1" height="1" viewBox="0 0 1 1"><circle r="1" fill="#000000"/></svg>"##;
        assert!(shapes(circle).is_err());
    }

    /// **The lockup is drawn where the layout puts it**: inked inside its
    /// box and nowhere outside it, in its two colours. Perturbation: leave
    /// the view box's origin out and the ink lands off the box.
    #[test]
    fn the_lockup_is_inked_inside_its_box() {
        let s = Strip::new(WIDE, 2.0, VERSION);
        let (lx, ly, lw, lh) = s.layout.lockup;
        let mut amber = 0;
        let mut word = 0;
        for (i, &p) in s.base.iter().enumerate() {
            let (x, y) = ((i % slots!(WIDE)) as f64, (i / slots!(WIDE)) as f64);
            let inside = x + 1.0 >= lx && x <= lx + lw && y + 1.0 >= ly && y <= ly + lh;
            if (p == 0xFFF4_A32C || p == 0xFFF3_F4F6) && !inside {
                panic!("lockup ink at {x},{y} outside {:?}", s.layout.lockup);
            }
            amber += usize::from(p == 0xFFF4_A32C);
            word += usize::from(p == 0xFFF3_F4F6);
        }
        assert!(amber > 200 && word > 200, "amber {amber}, word {word}");
    }
}
