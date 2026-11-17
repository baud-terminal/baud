//! **What the see-through standard material draws over a black and a white
//! backdrop, within the window**: `NSVisualEffectView` with
//! `.underWindowBackground` (material 21), `.withinWindow` blending and
//! the active state, over a layer filled with one grey, rendered offscreen
//! by `CARenderer` into a Metal texture and read back, under the dark and
//! the light appearance. The figure is the within-window variant: the
//! window server composites `.behindWindow` from the screen, and nothing
//! here reaches that.
//!
//! No window and no application: views are made and never put in a
//! window, `NSApplication` is never asked for, and
//! `tests/material_probe_gate.rs` reads this file and fails if it names
//! either. `harness = false` because AppKit views are main-thread-only;
//! this `main` is the process's main thread. It runs only when asked for
//! with `--ignored`, as a measurement does, and prints one line a case.
//!
//! The control, before any material is read: the same render with the
//! material hidden must give back the backdrop's grey, or the renderer is
//! not drawing the view's layers at all and nothing after it is a reading.

const NAME: &str = "the_standard_material_over_black_and_white_within_the_window";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ignored = args
        .iter()
        .any(|a| a == "--ignored" || a == "--include-ignored");
    if args.iter().any(|a| a == "--list") {
        if cfg!(target_os = "macos") && ignored {
            println!("{NAME}: test");
        }
        return;
    }
    let filter = args.iter().find(|a| !a.starts_with('-'));
    let runs = ignored && filter.is_none_or(|f| NAME.contains(f.as_str()));
    #[cfg(target_os = "macos")]
    if runs {
        the_standard_material_over_black_and_white_within_the_window();
        println!("test {NAME} ... ok");
    }
    #[cfg(not(target_os = "macos"))]
    let _ = runs;
}

/// The side of the square rendered, in points and texels.
#[cfg(target_os = "macos")]
const SIDE: usize = 64;

/// The same side as a length in points.
#[cfg(target_os = "macos")]
const SIDE_PT: f64 = 64.0;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn the_standard_material_over_black_and_white_within_the_window() {
    use macos_liquid_glass::objc2::MainThreadOnly;
    use macos_liquid_glass::objc2::rc::Retained;
    use macos_liquid_glass::objc2::runtime::AnyObject;
    use macos_liquid_glass::objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
        NSVisualEffectView,
    };
    use macos_liquid_glass::objc2_foundation::{
        MainThreadMarker, NSDictionary, NSPoint, NSRect, NSSize, NSString,
    };
    use objc2_core_graphics::CGColor;
    use objc2_metal::{
        MTLCommandBuffer, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLOrigin,
        MTLPixelFormat, MTLRegion, MTLSize, MTLStorageMode, MTLTexture, MTLTextureDescriptor,
        MTLTextureUsage,
    };
    use objc2_quartz_core::{
        CACurrentMediaTime, CALayer, CARenderer, CATransaction, kCARendererMetalCommandQueue,
    };
    use std::ffi::c_void;
    use std::ptr::NonNull;

    let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
    let device = MTLCreateSystemDefaultDevice().expect("a Metal device");
    let queue = device.newCommandQueue().expect("a command queue");
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIDE_PT, SIDE_PT));

    // The centre texel of one offscreen render of `layer`, as (red, green,
    // blue). The layer is handed to the renderer inside a transaction that
    // is committed and flushed before the frame begins, so the frame reads
    // the tree as set.
    let draw = |layer: &CALayer| -> (u8, u8, u8) {
        // SAFETY: the descriptor constructor takes plain values.
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::BGRA8Unorm,
                SIDE,
                SIDE,
                false,
            )
        };
        desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        desc.setStorageMode(MTLStorageMode::Shared);
        let texture = device.newTextureWithDescriptor(&desc).expect("a texture");
        // SAFETY: Core Animation's constant, read once.
        let key: &NSString = unsafe { kCARendererMetalCommandQueue };
        let value: &AnyObject = AsRef::<AnyObject>::as_ref(&*queue);
        let options = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[value]);
        // SAFETY: an `NSDictionary`'s type parameters are erased at run
        // time; the renderer reads the one key it documents.
        let options: Retained<NSDictionary> = unsafe { Retained::cast_unchecked(options) };
        // SAFETY: the texture is kept alive for the renderer's life, and
        // the options are the documented key with a queue on the
        // texture's device.
        let renderer =
            unsafe { CARenderer::rendererWithMTLTexture_options(&texture, Some(&options)) };
        CATransaction::begin();
        renderer.setLayer(Some(layer));
        renderer.setBounds(frame);
        CATransaction::commit();
        CATransaction::flush();
        // SAFETY: a null timestamp is the documented "none given".
        unsafe { renderer.beginFrameAtTime_timeStamp(CACurrentMediaTime(), std::ptr::null_mut()) };
        renderer.addUpdateRect(frame);
        renderer.render();
        renderer.endFrame();
        // The renderer encodes on `queue`: a buffer committed after it
        // completes after it.
        let fence = queue.commandBuffer().expect("a command buffer");
        fence.commit();
        fence.waitUntilCompleted();

        let mut texels = vec![0u32; SIDE * SIDE];
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: SIDE,
                height: SIDE,
                depth: 1,
            },
        };
        // SAFETY: `texels` holds SIDE x SIDE words, the region is the
        // texture's own extent at four bytes a texel.
        unsafe {
            texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                NonNull::from(texels.as_mut_slice()).cast::<c_void>(),
                SIDE * 4,
                region,
                0,
            );
        }
        let [b, g, r, _] = texels[(SIDE / 2) * SIDE + SIDE / 2].to_le_bytes();
        (r, g, b)
    };

    // One render of `grey` with the material over it (or hidden), under
    // `dark` or light; the centre texel, as (red, green, blue).
    let render = |grey: f64, dark: bool, shown: bool| -> (u8, u8, u8) {
        let page = NSView::initWithFrame(NSView::alloc(mtm), frame);
        page.setWantsLayer(true);
        let base = page.layer().expect("a layer-backed view has a layer");
        base.setBackgroundColor(Some(&CGColor::new_generic_gray(grey, 1.0)));
        let material = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
        material.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
        material.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        material.setState(NSVisualEffectState::Active);
        // SAFETY: the two names are AppKit's constants, read once.
        let name = unsafe {
            if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        let look = NSAppearance::appearanceNamed(name).expect("a named system appearance");
        material.setAppearance(Some(&look));
        page.setAppearance(Some(&look));
        material.setWantsLayer(true);
        material.setHidden(!shown);
        page.addSubview(&material);
        page.layoutSubtreeIfNeeded();
        page.displayIfNeeded();
        CATransaction::flush();
        draw(&base)
    };

    // The first control: a bare layer, no view, filled white. If this
    // comes back black the renderer drew nothing into the texture, and no
    // view or material reading below can mean anything.
    let bare = CALayer::new();
    bare.setBounds(frame);
    bare.setBackgroundColor(Some(&CGColor::new_generic_gray(1.0, 1.0)));
    let got = draw(&bare);
    println!("control: a bare layer filled white -> {got:?}");
    assert!(
        [got.0, got.1, got.2].iter().all(|&c| c >= 254),
        "the renderer drew nothing into the texture: {got:?} for a bare white layer"
    );
    // The second control: the material hidden, the backdrop comes back as
    // given, so the view's own layer is drawn.
    for (grey, want) in [(0.0, 0u8), (1.0, 255u8)] {
        let got = render(grey, true, false);
        println!("control: backdrop {grey:.0}, material hidden -> {got:?}");
        assert!(
            [got.0, got.1, got.2].iter().all(|&c| c.abs_diff(want) <= 1),
            "the renderer did not draw the view's own layer: {got:?} for grey {grey}"
        );
    }
    // The readings: each appearance over each end of the desktop.
    let mut drew = false;
    for dark in [true, false] {
        for (grey, bare) in [(0.0, 0u8), (1.0, 255u8)] {
            let got = render(grey, dark, true);
            let look = if dark { "dark" } else { "light" };
            println!(
                "material 21 within-window, {look}, over {grey:.0}: rgb {} {} {}",
                got.0, got.1, got.2
            );
            drew |= [got.0, got.1, got.2].iter().any(|&c| c.abs_diff(bare) > 1);
        }
    }
    println!(
        "material drawn: {}",
        if drew {
            "yes"
        } else {
            "NO (every reading is the bare backdrop)"
        }
    );
}
