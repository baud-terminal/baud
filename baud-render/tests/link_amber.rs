//! **A link's line at rest is `[message]`'s box colour**, on the CPU path,
//! which the Linux driver draws with: by default baud's amber, `#F4A32C`,
//! read from the brand sheet's generator, on both built-in themes' pages and
//! on each reversed, never a darker shade, whatever that costs against the
//! page; a `box_color` of the user's moves it. The ratios are WCAG 2.2's
//! (`[[source: wcag22]]`), over the line's drawn pixels against the page
//! drawn beside it, and they are the figures MEASUREMENTS.md's "The link
//! line in amber" states: two of them are under 1.4.11's 3:1.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Blend, Damage, LineStyle, LinkLine, MessageColours, MessagePaint, Metrics, PxSize,
    Renderer, Stack, Target, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 4,
};

/// The URL's cells and the printed path's, on row 0.
const URL: (usize, usize) = (4, 24);
const PATH: (usize, usize) = (30, 39);

/// The brand sheet's amber in the brand generator,
/// `tools/baud-pictures/src/brand.rs` (`pub const AMBER: &str = "#F4A32C";`).
fn amber() -> Argb {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/baud-pictures/src/brand.rs"
    );
    let text = std::fs::read_to_string(path).expect("the brand generator");
    let at = "\npub const AMBER: &str = \"#";
    let start = text.find(at).expect("AMBER in it") + at.len();
    Argb(0xFF00_0000 | u32::from_str_radix(&text[start..start + 6], 16).expect("six hex digits"))
}

/// WCAG 2.2's relative luminance and contrast ratio, from its definitions.
fn ratio(a: Argb, b: Argb) -> f64 {
    let lum = |c: Argb| {
        let c = c.0;
        let f = |v: u32| {
            let v = f64::from(v & 0xFF) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c >> 16) + 0.7152 * f(c >> 8) + 0.0722 * f(c)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// A frame and where to read it: its width and the renderer's cell.
struct Drawn {
    px: Vec<u32>,
    w: baud_font::Px,
    m: Metrics,
}

impl Drawn {
    fn at(&self) -> (usize, usize, usize) {
        let n = |v: baud_font::Px| usize::try_from(v.value()).unwrap();
        (n(self.w), n(self.m.cell_w), n(self.m.underline))
    }

    /// The line's row of pixels over `span`, opaque.
    fn row(&self, (from, to): (usize, usize)) -> Vec<Argb> {
        let (stride, cw, y) = self.at();
        (from * cw..(to + 1) * cw)
            .map(|x| Argb(self.px[y * stride + x] | 0xFF00_0000))
            .collect()
    }

    /// The page on the line's row, past every link, opaque.
    fn page(&self) -> Argb {
        let (stride, cw, y) = self.at();
        Argb(self.px[y * stride + 60 * cw] | 0xFF00_0000)
    }
}

/// A URL and a printed path on `theme`'s page, `video` after them, drawn with
/// and without their lines at rest.
fn drawn(theme: Theme, video: &[u8], colours: MessageColours) -> (Drawn, Drawn) {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = [
        b"\x1b[?25lsee https://example.org/x and /etc/hosts".as_slice(),
        video,
    ]
    .concat();
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(4),
    )
    .unwrap();
    r.fit(&t);
    r.set_message_colours(colours);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let m = r.metrics();
    let frame = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>| {
        let mut px = vec![0u32; n];
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(t, &mut target, Damage::Full);
        Drawn { px, w, m }
    };
    let bare = frame(&mut r, &mut t);
    let word = |x: usize| {
        let c = t.view(Rows::ZERO).row(Row::ZERO)[x];
        t.styles().one().word_of(c)
    };
    let lines = [URL, PATH].map(|(from, to)| LinkLine {
        row: Row::ZERO,
        from: Col::at_index(from),
        to: Col::at_index(to),
        word: word(from),
    });
    r.set_link_lines(&lines, LineStyle::Solid);
    let lined = frame(&mut r, &mut t);
    (bare, lined)
}

/// The line's pixels over `span` where no glyph's ink reaches its row in the
/// frame without lines: most of them, all `want`.
fn assert_line(bare: &Drawn, lined: &Drawn, span: (usize, usize), want: Argb, what: &str) {
    let page = bare.page();
    let clear: Vec<Argb> = bare
        .row(span)
        .iter()
        .zip(lined.row(span))
        .filter(|(b, _)| **b == page)
        .map(|(_, l)| l)
        .collect();
    assert!(
        clear.len() * 2 > (span.1 + 1 - span.0) * bare.at().1,
        "{what}: clear"
    );
    assert!(
        clear.iter().all(|&p| p == want),
        "{what}: the line is #{:06X} across its cells, found #{:06X}",
        want.0 & 0x00FF_FFFF,
        clear[0].0 & 0x00FF_FFFF
    );
}

/// A built-in page: its name, its theme, the video set after the text, and
/// the line's ratio on it.
type Page = (&'static str, fn() -> Theme, &'static [u8], &'static str);

const PAGES: [Page; 4] = [
    ("dark", Theme::dark, b"", "9.17"),
    ("light", Theme::light, b"", "1.80"),
    ("dark reversed", Theme::dark, b"\x1b[?5h", "1.46"),
    ("light reversed", Theme::light, b"\x1b[?5h", "7.20"),
];

/// **(a) and (b): the line at rest is the brand's amber on every built-in
/// page, a URL's and a printed path's, and its ratio against the page is the
/// one MEASUREMENTS.md states**: 9.17 on dark, 1.80 on light, 1.46 on dark
/// reversed, 7.20 on light reversed, read from the drawn pixels. The control:
/// the page under the line is the theme's own. Perturbations: draw the line
/// in the text's colour (`Renderer::place_links_with`'s `rest`); a darker
/// shade on light, which the pinned 1.80 refuses.
#[test]
fn the_line_at_rest_is_the_brand_amber_on_every_page() {
    let amber = amber();
    assert_eq!(amber, Argb(0xFFF4_A32C), "the brand's amber");
    for (name, theme, video, pinned) in PAGES {
        let (bare, lined) = drawn(theme(), video, MessageColours::default());
        assert_line(&bare, &lined, URL, amber, &format!("{name}, the URL"));
        assert_line(&bare, &lined, PATH, amber, &format!("{name}, the path"));
        let line = lined.row(URL)[0];
        let page = lined.page();
        assert_eq!(page, bare.page(), "{name}: the page is the theme's");
        assert_eq!(
            format!("{:.2}", ratio(line, page)),
            pinned,
            "{name}: {line:?} on {page:?}"
        );
    }
}

/// **(c): a `box_color` of the user's moves the line**: `#3366CC`, with a
/// white text that reads on it, on dark and on light. Perturbation: a
/// hard-coded amber.
#[test]
fn a_box_color_of_the_users_moves_the_line() {
    let colours = MessageColours {
        page: MessagePaint::Colour(Argb(0xFF33_66CC)),
        text: MessagePaint::Colour(Argb(0xFFFF_FFFF)),
    };
    for (name, theme, video, _) in PAGES {
        let (bare, lined) = drawn(theme(), video, colours);
        let blue = Argb(0xFF33_66CC);
        assert_line(&bare, &lined, URL, blue, &format!("{name}, the URL"));
        assert_line(&bare, &lined, PATH, blue, &format!("{name}, the path"));
    }
}
