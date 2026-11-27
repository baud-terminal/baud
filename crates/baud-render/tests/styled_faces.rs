//! Bold and italic text is drawn from the real Bold, Italic and Bold Italic
//! faces, never faked, on the CPU path; the cell shader's half is the pixel
//! oracle in `baud-present-macos`.
//!
//! The oracle for "drawn from the bold face" is the bold face itself: a
//! bold cell on a stack whose Bold face is a styled face must paint exactly
//! what the same text paints with the Bold file as the only text face. The
//! control that the bold face reached the pixels at all is that the same
//! bold cell on a stack with no Bold face paints differently.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, FaceRole, FaceStyle, PxSize, Renderer, Stack, Theme};

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 16,
};

/// A font under `crates/baud-app/fonts/`. The text styles here are stock JetBrains
/// Mono's: the embedded Regular and Bold are those files with Hebrew added,
/// and this text is Latin and an icon, which the two draw alike.
fn app_font(path: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../baud-app/fonts/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("{path}: {e}"))
}

struct Files {
    regular: Vec<u8>,
    bold: Vec<u8>,
    italic: Vec<u8>,
    bold_italic: Vec<u8>,
    symbols: Vec<u8>,
}

impl Files {
    fn load() -> Files {
        Files {
            regular: app_font("jetbrains-mono/JetBrainsMono-Regular.ttf"),
            bold: app_font("jetbrains-mono/JetBrainsMono-Bold.ttf"),
            italic: app_font("jetbrains-mono/JetBrainsMono-Italic.ttf"),
            bold_italic: app_font("jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
            symbols: app_font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
        }
    }

    fn of(&self, style: FaceStyle) -> &[u8] {
        match style {
            FaceStyle::Regular => &self.regular,
            FaceStyle::Bold => &self.bold,
            FaceStyle::Italic => &self.italic,
            FaceStyle::BoldItalic => &self.bold_italic,
        }
    }
}

/// The pixels `bytes` paints through a stack of `faces` with `roles`,
/// incrementally after `before` when it is given.
fn painted(
    faces: &[Face<'_>],
    roles: &[FaceRole],
    before: Option<&[u8]>,
    bytes: &[u8],
) -> Vec<u32> {
    let mut r = Renderer::new(
        Stack::with_roles(faces, roles).unwrap(),
        PxSize::of(16.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);
    let feed = |t: &mut Term<'_>, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    };
    let damage = match before {
        Some(b) => {
            feed(&mut t, b);
            support::paint(&mut r, &mut t, &mut c, Damage::Full);
            Damage::Incremental
        }
        None => Damage::Full,
    };
    feed(&mut t, bytes);
    support::paint(&mut r, &mut t, &mut c, damage);
    c.pixels
}

/// Text, a ligature JetBrains Mono forms through `calt`, and a Nerd Fonts
/// icon the text faces lack, in a truecolour grey so bold's brightening of
/// palette 0-7 cannot be what differs.
const TEXT: &[u8] = b"ab->x \xef\x81\xbb";

fn sgr(style: FaceStyle) -> &'static [u8] {
    match style {
        FaceStyle::Regular => b"\x1b[0;38;2;200;200;200m",
        FaceStyle::Bold => b"\x1b[0;1;38;2;200;200;200m",
        FaceStyle::Italic => b"\x1b[0;3;38;2;200;200;200m",
        FaceStyle::BoldItalic => b"\x1b[0;1;3;38;2;200;200;200m",
    }
}

fn line(style: FaceStyle) -> Vec<u8> {
    [sgr(style), TEXT].concat()
}

const STYLED: [FaceRole; 5] = [
    FaceRole::Fallback,
    FaceRole::Fallback,
    FaceRole::Style(FaceStyle::Bold),
    FaceRole::Style(FaceStyle::Italic),
    FaceRole::Style(FaceStyle::BoldItalic),
];

/// Each style paints what its own file paints as the only text face, with
/// the icon from the icon face behind it; and differs from the same styled
/// text on a stack that has no face for the style.
#[test]
fn a_styled_cell_draws_its_own_face() {
    let f = Files::load();
    let faces: Vec<Face<'_>> = [&f.regular, &f.symbols, &f.bold, &f.italic, &f.bold_italic]
        .iter()
        .map(|d| Face::parse(d).unwrap())
        .collect();
    let plain = [FaceRole::Fallback, FaceRole::Fallback];
    let regular_only: Vec<Face<'_>> = [&f.regular, &f.symbols]
        .iter()
        .map(|d| Face::parse(d).unwrap())
        .collect();
    for style in [FaceStyle::Bold, FaceStyle::Italic, FaceStyle::BoldItalic] {
        let got = painted(&faces, &STYLED, None, &line(style));
        let alone: Vec<Face<'_>> = [f.of(style), &f.symbols]
            .iter()
            .map(|d| Face::parse(d).unwrap())
            .collect();
        let want = painted(&alone, &plain, None, &line(FaceStyle::Regular));
        assert!(got == want, "{style:?} does not paint its own face");
        let faked = painted(&regular_only, &plain, None, &line(style));
        assert!(
            got != faked,
            "{style:?}: the control -- a stack with no face for it paints the same"
        );
    }
    // Regular text on the styled stack is the regular face's.
    let got = painted(&faces, &STYLED, None, &line(FaceStyle::Regular));
    let want = painted(&regular_only, &plain, None, &line(FaceStyle::Regular));
    assert!(got == want, "regular text left the regular face");
}

/// A row mixing styles splits its runs by face: each cell paints what its
/// own style's line paints in that cell. Spaced apart so an italic
/// overhang cannot reach a neighbour.
#[test]
fn a_row_mixing_styles_draws_each_cell_from_its_face() {
    let f = Files::load();
    let faces: Vec<Face<'_>> = [&f.regular, &f.symbols, &f.bold, &f.italic, &f.bold_italic]
        .iter()
        .map(|d| Face::parse(d).unwrap())
        .collect();
    let mut mixed = Vec::new();
    for style in FaceStyle::ALL {
        mixed.extend_from_slice(sgr(style));
        mixed.extend_from_slice(b"a  ");
    }
    let got = painted(&faces, &STYLED, None, &mixed);
    let r = Renderer::new(
        Stack::with_roles(&faces, &STYLED).unwrap(),
        PxSize::of(16.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let cw = usize::try_from(r.metrics().cell_w.value()).unwrap();
    let (w, _) = r.pixel_size();
    let w = usize::try_from(w.value()).unwrap();
    for (i, style) in FaceStyle::ALL.into_iter().enumerate() {
        // `CSI 0 C` moves one column, as `CSI C` does, so the first cell
        // takes no move at all.
        let to = if i == 0 {
            String::new()
        } else {
            format!("\x1b[{}C", 3 * i)
        };
        let alone: Vec<u8> = [to.as_bytes(), sgr(style), b"a"].concat();
        let want = painted(&faces, &STYLED, None, &alone);
        let col = |px: &[u32]| -> Vec<u32> {
            px.chunks(w)
                .flat_map(|row| row[3 * i * cw..(3 * i + 1) * cw].to_vec())
                .collect()
        };
        assert!(
            col(&got) == col(&want),
            "cell {i} ({style:?}) is not its own face's"
        );
    }
}

/// Turning a line bold changes no cell's content, only its rendition, and
/// an incremental frame paints what a full one does.
#[test]
fn a_style_change_repaints_incrementally_to_what_a_full_frame_paints() {
    let f = Files::load();
    let faces: Vec<Face<'_>> = [&f.regular, &f.symbols, &f.bold, &f.italic, &f.bold_italic]
        .iter()
        .map(|d| Face::parse(d).unwrap())
        .collect();
    let first = line(FaceStyle::Regular);
    let then = [b"\x1b[H".as_slice(), &line(FaceStyle::BoldItalic)].concat();
    let incremental = painted(&faces, &STYLED, Some(&first), &then);
    let full = painted(&faces, &STYLED, None, &[first.as_slice(), &then].concat());
    assert!(incremental == full);
    let before = painted(&faces, &STYLED, None, &first);
    assert!(
        incremental != before,
        "the control: the restyle reached the pixels"
    );
}
