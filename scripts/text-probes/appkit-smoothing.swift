// appkit-smoothing: what AppleFontSmoothing does to AppKit's text.
//
//   for v in 0 1 2 3; do ./appkit-smoothing -AppleFontSmoothing $v; done; ./appkit-smoothing
//
// An NSAttributedString drawn white on black into a bitmap through
// NSGraphicsContext, the preference set in the argument domain so the user's
// settings are untouched; prints the ink. Measured: absent, 1, 2 and 3 give the
// same smoothed ink and 0 the plain one -- and a bare CGContext, drawing with
// CTFontDrawGlyphs, gives the smoothed ink whatever the key says.
import AppKit
let W = 64, H = 64
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: W, pixelsHigh: H, bitsPerSample: 8,
                           samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 4 * W, bitsPerPixel: 32)!
let ctx = NSGraphicsContext(bitmapImageRep: rep)!
NSGraphicsContext.current = ctx
NSColor.black.setFill(); NSRect(x: 0, y: 0, width: W, height: H).fill()
let s = NSAttributedString(string: "H", attributes: [.font: NSFont(name: "Menlo", size: 28)!,
                                                      .foregroundColor: NSColor.white])
s.draw(at: NSPoint(x: 16, y: 8))
ctx.flushGraphics()
let b = rep.bitmapData!
let ink = (0..<(W * H)).reduce(0) { $0 + Int(b[4 * $1]) }
let pref = UserDefaults.standard.object(forKey: "AppleFontSmoothing").map { "\($0)" } ?? "absent"
print("AppleFontSmoothing=\(pref): AppKit ink \(ink)")
