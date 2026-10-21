//! A print job on the system's print panel: the pages `baud-render`'s
//! printer laid out, as text views on sheets of the panel's paper, and the
//! panel shown as a sheet on the window that printed.
//!
//! **The panel always shows** (`[[source:
//! apple-nsprintoperation-showsprintpanel]]`, "A Boolean value that
//! determines whether the print operation displays a print panel"): nothing
//! reaches paper until the user presses Print, so a program can never print
//! by itself. **And it is document-modal**, run with
//! `runOperationModalForWindow:delegate:didRunSelector:contextInfo:` for the
//! window that printed, "The document window to receive a print progress
//! sheet" (`[[source: apple-nsprintoperation-runmodal]]`), with no delegate:
//! `runOperation` "runs to completion in the current thread, blocking the
//! application" (`[[source: apple-nsprintoperation-run]]`), and the main
//! thread is the one that feeds the core and draws.
//!
//! `unsafe` is allowed here, as in [`crate::metal`], for three calls the
//! bindings mark so: the text view's storage and container, an attribute's
//! value, and the sheet's run, whose delegate, selector and context are all
//! nothing.
#![allow(unsafe_code)]

use baud_render::print::{Layout, Look, Page, Run, Scale};
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use macos_liquid_glass::objc2_app_kit::{
    NSColor, NSFont, NSFontWeightBold, NSFontWeightRegular, NSMutableParagraphStyle, NSPrintInfo,
    NSPrintOperation, NSPrinter, NSPrintingPaginationMode, NSTextView, NSView, NSWindow,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{
    NSMutableAttributedString, NSNumber, NSPoint, NSRange, NSRect, NSSize, NSString,
};

/// Whether macOS lists a printer, "the names of all available printers"
/// (`[[source: apple-nsprinter-printernames]]`): what DSR `?15` answers
/// from (note AW).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Listed {
    Some,
    None,
}

/// The printers macOS lists, read when asked: a printer added while baud
/// runs is seen at the next ask. **Callable off the main thread**:
/// `NSPrinter` is declared without `@MainActor` (`[[source:
/// apple-nsprinter]]`), where `NSView` and `NSWindow` carry it, and is in
/// neither of AppKit's restricted lists, the thread-unsafe classes and
/// those "used only from the main thread" (`[[source:
/// apple-thread-safety-summary]]`). In a pool of its own, which a thread
/// AppKit does not run has none of.
pub fn printers() -> Listed {
    macos_liquid_glass::objc2::rc::autoreleasepool(|_| match NSPrinter::printerNames().count() {
        0 => Listed::None,
        _ => Listed::Some,
    })
}

/// The sheet's printable width and height in points: the shared print
/// info's paper less its margins, what the panel starts from.
pub fn sheet_area() -> (CGFloat, CGFloat) {
    let info = NSPrintInfo::sharedPrintInfo();
    let paper = info.paperSize();
    (
        paper.width - info.leftMargin() - info.rightMargin(),
        paper.height - info.topMargin() - info.bottomMargin(),
    )
}

/// The system monospaced font's advance and line height, each per point of
/// size, measured from the font rather than assumed.
pub fn font_metrics() -> (CGFloat, CGFloat) {
    let size: CGFloat = 100.0;
    // SAFETY: an extern static of the bindings, a `CGFloat` read once.
    let font = NSFont::monospacedSystemFontOfSize_weight(size, unsafe { NSFontWeightRegular });
    let advance = font.maximumAdvancement().width / size;
    let leading = (font.ascender() - font.descender() + font.leading()) / size;
    (advance, leading)
}

/// The attribute names, spelled as their values so no extern static is
/// read for each run.
const FONT: &str = "NSFont";
const COLOR: &str = "NSColor";
const BACKGROUND: &str = "NSBackgroundColor";
const UNDERLINE: &str = "NSUnderline";
const EXPANSION: &str = "NSExpansion";
const PARAGRAPH: &str = "NSParagraphStyle";
const KERN: &str = "NSKern";

/// One page's lines as attributed text at `layout`'s size: bold in the
/// font's bold weight, underline single, reverse white on black, blink
/// plain; a double-width line stretched to twice its width, a double-height
/// one at twice the size over two lines' height. **A character struck over
/// is drawn under what struck it**: the run before carries a negative kern
/// on its last character, the columns it goes back, so the next glyph
/// starts where it did (`Run::kern`).
fn page_text(page: &Page, layout: Layout) -> Retained<NSMutableAttributedString> {
    let out = NSMutableAttributedString::new();
    for line in &page.lines {
        let (size, expand, height) = match line.scale {
            Scale::Single => (layout.size, 0.0, layout.line_height),
            Scale::DoubleWidth => (layout.size, core::f64::consts::LN_2, layout.line_height),
            Scale::DoubleTop | Scale::DoubleBottom => {
                (layout.size * 2.0, 0.0, layout.line_height * 2.0)
            }
        };
        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setMinimumLineHeight(height);
        paragraph.setMaximumLineHeight(height);
        let mut runs = line.runs();
        runs.push(Run::new("\n", Look::PLAIN));
        for r in runs {
            let look = r.look;
            let run = NSMutableAttributedString::initWithString(
                NSMutableAttributedString::alloc(),
                &NSString::from_str(&r.text),
            );
            let all = NSRange::new(0, run.length());
            let kern = r.kern(layout, line.scale).0;
            let last = r.text.chars().next_back().map_or(0, char::len_utf16);
            // SAFETY: extern statics of the bindings, `CGFloat`s read.
            let weight = match look.has(Look::BOLD) {
                true => unsafe { NSFontWeightBold },
                false => unsafe { NSFontWeightRegular },
            };
            let font = NSFont::monospacedSystemFontOfSize_weight(size, weight);
            let (ink, paper) = match look.has(Look::REVERSE) {
                true => (NSColor::whiteColor(), Some(NSColor::blackColor())),
                false => (NSColor::blackColor(), None),
            };
            // SAFETY: each value is of the type its attribute takes -- a
            // font, a colour, a number, a paragraph style -- and `all` is
            // the run's whole length, `end` its last character's.
            unsafe {
                run.addAttribute_value_range(&NSString::from_str(FONT), &font, all);
                run.addAttribute_value_range(&NSString::from_str(COLOR), &ink, all);
                run.addAttribute_value_range(&NSString::from_str(PARAGRAPH), &paragraph, all);
                if let Some(paper) = paper {
                    run.addAttribute_value_range(&NSString::from_str(BACKGROUND), &paper, all);
                }
                if look.has(Look::UNDERLINE) {
                    let single = NSNumber::new_isize(1);
                    run.addAttribute_value_range(&NSString::from_str(UNDERLINE), &single, all);
                }
                if expand != 0.0 {
                    let wide = NSNumber::new_f64(expand);
                    run.addAttribute_value_range(&NSString::from_str(EXPANSION), &wide, all);
                }
                if kern != 0.0 && last > 0 {
                    let back = NSNumber::new_f64(kern);
                    let end = NSRange::new(all.length - last, last);
                    run.addAttribute_value_range(&NSString::from_str(KERN), &back, end);
                }
            }
            out.appendAttributedString(&run);
        }
    }
    out
}

/// **The pages as one view, each a sheet's height**, the first at the top:
/// a print of a view that is taller than a sheet goes on to the next sheet
/// at the sheet's height, so each page's text view starts a sheet.
fn pages_view(pages: &[Page], layout: Layout, mtm: MainThreadMarker) -> Retained<NSView> {
    let (width, height) = sheet_area();
    let total = height * pages.len().max(1) as CGFloat;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, total));
    let root = NSView::initWithFrame(NSView::alloc(mtm), frame);
    for (i, page) in pages.iter().enumerate() {
        let y = total - height * (i as CGFloat + 1.0);
        let at = NSRect::new(NSPoint::new(0.0, y), NSSize::new(width, height));
        let view = NSTextView::initWithFrame(NSTextView::alloc(mtm), at);
        view.setEditable(false);
        view.setDrawsBackground(false);
        // SAFETY: a text view made here has its storage and container;
        // neither is kept past this function.
        unsafe {
            if let Some(container) = view.textContainer() {
                container.setLineFragmentPadding(0.0);
                container.setWidthTracksTextView(false);
                container.setSize(NSSize::new(CGFloat::MAX, CGFloat::MAX));
            }
            if let Some(storage) = view.textStorage() {
                storage.setAttributedString(&page_text(page, layout));
            }
        }
        root.addSubview(&view);
    }
    root
}

/// **The print panel for `pages`, as a sheet on `window`**, named `title`
/// so the panel says which window printed. Returns as soon as the sheet is
/// up; the window's `attachedSheet` says when it has gone.
pub fn show_panel(
    window: &NSWindow,
    title: &str,
    pages: &[Page],
    layout: Layout,
    mtm: MainThreadMarker,
) {
    let info = NSPrintInfo::sharedPrintInfo();
    info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
    info.setHorizontalPagination(NSPrintingPaginationMode::Clip);
    info.setVerticallyCentered(false);
    info.setHorizontallyCentered(false);
    let view = pages_view(pages, layout, mtm);
    let op = NSPrintOperation::printOperationWithView_printInfo(&view, &info);
    op.setShowsPrintPanel(true);
    op.setShowsProgressPanel(true);
    op.setJobTitle(Some(&NSString::from_str(title)));
    // SAFETY: no delegate, so no selector is sent and the context is never
    // read; the window outlives the call, which returns with the sheet up.
    unsafe {
        op.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
            window,
            None,
            None,
            core::ptr::null_mut(),
        );
    }
}
