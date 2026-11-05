//! **An SVG drawn as `resvg --skip-system-fonts [-w N -h N] in.svg
//! out.png` draws it, read back as Pillow reads that PNG**: resvg 0.48.1
//! as a library with the options its command line builds by default
//! (`src/main.rs`'s `parse_args`), the pixmap fitted and scaled as its
//! `FitTo::Size` does, and the premultiplied pixels demultiplied as its
//! `save_png` demultiplies them (`Pixmap::take_demultiplied`), which is
//! the RGBA Pillow loads. Measured: the library's PNG and the pinned
//! command's are the same bytes for every picture the brand draws.

use crate::png::{Image, Mode};
use resvg::tiny_skia::{IntSize, Pixmap, Transform};
use resvg::usvg;

/// The command line's `usvg::Options` for an SVG read from `dir`, with no
/// fonts loaded: the SVGs here hold no text.
fn options(size: Option<u32>) -> usvg::Options<'static> {
    let default_size = match size {
        Some(n) => usvg::Size::from_wh(n as f32, n as f32),
        None => usvg::Size::from_wh(100.0, 100.0),
    }
    .expect("a positive size");
    usvg::Options {
        resources_dir: None,
        dpi: 96.0,
        font_family: "Times New Roman".to_string(),
        font_size: 12.0,
        languages: vec!["en".to_string()],
        shape_rendering: usvg::ShapeRendering::default(),
        text_rendering: usvg::TextRendering::default(),
        image_rendering: usvg::ImageRendering::default(),
        default_size,
        ..usvg::Options::default()
    }
}

/// The SVG drawn, at its own size or fitted to `size` by `size`.
///
/// # Errors
/// When resvg cannot read the SVG.
pub fn draw(svg: &str, size: Option<u32>) -> Result<Image, String> {
    let tree = usvg::Tree::from_str(svg, &options(size)).map_err(|e| e.to_string())?;
    let own = tree.size().to_int_size();
    let fit = match size {
        Some(n) => own.scale_to(IntSize::from_wh(n, n).ok_or("a zero size")?),
        None => own,
    };
    let mut pm = Pixmap::new(fit.width(), fit.height()).ok_or("no pixmap that size")?;
    let (a, b) = (own.to_size(), fit.to_size());
    let ts = Transform::from_scale(b.width() / a.width(), b.height() / a.height());
    resvg::render(&tree, ts, &mut pm.as_mut());
    Ok(Image {
        w: fit.width() as usize,
        h: fit.height() as usize,
        mode: Mode::Rgba,
        px: pm.take_demultiplied(),
    })
}
