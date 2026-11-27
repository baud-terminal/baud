//! `pixels`: one frame of a byte stream, as the Linux app draws it, written
//! to a file, so two systems' frames can be compared pixel for pixel
//! (`tools/scripts/linux-pixels.sh`).
//!
//! Usage: `pixels <stream> <out.pam>`. The stream is fed whole, then one
//! full frame is painted on the CPU path and written as a PAM
//! (`TUPLTYPE RGB_ALPHA`), each pixel the renderer's straight-alpha word
//! as R, G, B, A. Nothing is timed.
//!
//! The setup is the Linux driver's with the default config (`linux.rs`'s
//! `run`, `config.rs`'s `Config::defaults`), where no system face is
//! installed: the embedded Regular, icon face and STIX Two Math in the
//! chain, the embedded Bold, Italic and Bold Italic as the styled faces and
//! the Bold as the badge's; 13 points, 13 px at a backing scale of one;
//! `Theme::dark()` at the dark glass dim, 0.70; `Blend::Native`; the
//! default widths and image budget. A face the app finds on the system
//! and this does not read is a different stack, which the comparison
//! reports as a different frame, not as a renderer's difference.
//!
//! `PIXELS_WS=<cols>x<rows>` sets the screen, 80x24 by default.
//! `PIXELS_SCALE=<pixels a point>` draws at that backing scale, as the
//! Linux driver draws at the scale its compositor prefers and the macOS
//! app at its backing scale: the 13 points of the font at that many pixels
//! each, and the core told the scale; 1 by default.
//! `PIXELS_BACKDROP=standard` reads the contrast floor against the standard
//! material's dark greys, as the macOS app's dark theme does (`app.rs`'s
//! `theme_for`), where the default is the Linux driver's black to white.
//! `PIXELS_ALPHA=<0-255>` sets the page's alpha in place of the dim.
//! `PIXELS_FACES=path[#index],...` puts these faces in the chain after
//! the embedded icon face, as installed system faces in the default stack
//! are: DejaVu Sans Mono and Noto Color Emoji on Linux, Menlo and the
//! five faces after it on macOS.

use baud_core::{Owned, Term};
use baud_domain::{
    AmbiguousWidth, BytePos, Chan, Cols, ImageBudget, ReadLen, Rows, Vs16Width, WinSize,
};
use baud_font::Face;
use baud_render::{Backdrop, Blend, Damage, FaceRole, PxSize, Renderer, Stack, Target, Theme};
use std::io::Write as _;

/// `host.rs`'s `IMAGE_BUDGET`, the default `[images]` budget.
const IMAGE_BUDGET: ImageBudget = ImageBudget { pixels: 95 << 18 };

fn main() {
    baud_bench::warm::refuse_while_warm();
    let mut args = baud_bench::args();
    let (Some(input), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: pixels <stream> <out.pam>");
        std::process::exit(2);
    };
    let data = std::fs::read(&input).expect("the stream");
    // `default_faces` on Linux, in its order (`baud_bench::linux_faces`),
    // with the system faces `PIXELS_FACES` names in place of those it finds.
    let mut system = Vec::new();
    if let Ok(spec) = std::env::var("PIXELS_FACES") {
        for entry in spec.split(',').filter(|p| !p.is_empty()) {
            let (path, index) = entry.split_once('#').unwrap_or((entry, "0"));
            system.push((
                std::fs::read(path).expect("a face file"),
                baud_font::FaceIndex::of(index.parse().expect("a face index")),
            ));
        }
    }
    let files = baud_bench::linux_faces(system);
    let faces: Vec<Face<'_>> = files
        .iter()
        .map(|(bytes, index, _)| Face::parse_index(bytes, *index).expect("a face"))
        .collect();
    let roles: Vec<FaceRole> = files.iter().map(|(_, _, r)| *r).collect();
    let ws = std::env::var("PIXELS_WS")
        .ok()
        .and_then(|v| {
            let (c, r) = v.split_once('x')?;
            Some(WinSize {
                ws_row: r.parse().ok()?,
                ws_col: c.parse().ok()?,
            })
        })
        .unwrap_or(WinSize {
            ws_row: 24,
            ws_col: 80,
        });
    let scale: f32 = std::env::var("PIXELS_SCALE")
        .ok()
        .map_or(1.0, |s| s.parse().expect("a scale"));
    let theme = Theme {
        bg_alpha: std::env::var("PIXELS_ALPHA")
            .ok()
            .and_then(|s| s.parse().ok())
            .map_or(Chan::from_unit(0.70), Chan::from_u8),
        backdrop: match std::env::var("PIXELS_BACKDROP").as_deref() {
            Ok("standard") => Backdrop::under_window_dark(),
            _ => Backdrop::any(),
        },
        ..Theme::dark()
    };
    let mut r = Renderer::new(
        Stack::with_roles(&faces, &roles).expect("the stack"),
        PxSize::of(13.0 * scale).expect("size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .expect("cell metrics");
    let mut owned = Owned::new(&ws).with_images(IMAGE_BUDGET);
    let mut t = Term::new(owned.storage(), &ws);
    let m = r.metrics();
    t.set_cell_size(
        baud_domain::ImagePx::of(u16::try_from(m.cell_w.value()).unwrap_or(u16::MAX)),
        baud_domain::ImagePx::of(u16::try_from(m.cell_h.value()).unwrap_or(u16::MAX)),
    );
    t.set_backing_scale(baud_domain::BackingScale::of(f64::from(scale)));
    t.set_vs16_width(Vs16Width::Two);
    t.set_ambiguous_width(AmbiguousWidth::Narrow);
    t.write_at(BytePos::ZERO, &data);
    t.feed(BytePos::ZERO, ReadLen::from_index(data.len()));
    r.fit(&t);
    r.place_marks(
        &t,
        t.window_back(),
        baud_render::Leaving::None,
        baud_font::Px::of(0),
    );
    let (w, h) = r.whole_size();
    let (wu, hu) = (
        usize::try_from(w.value()).expect("width"),
        usize::try_from(h.value()).expect("height"),
    );
    let mut frame = vec![r.clear_color_of(&t).0; wu * hu];
    let mut target = Target::new(&mut frame, w, w, h).expect("the target");
    let seal = t.seal_writes();
    r.render(&t, seal, &mut target, Damage::Full);
    let mut bytes =
        format!("P7\nWIDTH {wu}\nHEIGHT {hu}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n")
            .into_bytes();
    for p in &frame {
        let [a, r, g, b] = p.to_be_bytes();
        bytes.extend_from_slice(&[r, g, b, a]);
    }
    std::fs::File::create(&out)
        .and_then(|mut f| f.write_all(&bytes))
        .expect("the output");
    println!(
        "{input}: {wu}x{hu} px, cell {}x{}, {} faces",
        m.cell_w.value(),
        m.cell_h.value(),
        faces.len()
    );
}
