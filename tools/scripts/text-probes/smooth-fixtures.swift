// gen: CoreText font-smoothing fixtures for baud's dilation (JetBrains Mono Regular, printable ASCII).
//
//   gen <font.ttf> <out-dir>
//
// Glyphs: printable ASCII U+0021..U+007E and the block elements U+2580 U+2584 U+2588 U+258C U+2590
// (flat edges at every size, for the per-edge gate). One file per ppem, `ct-smooth-<ppem>.bin`, little-endian:
//   file:  "BDIL" | u16 version=1 | u16 glyph count | f32 ppem | u32 font byte length | u32 font FNV-1a
//          | u8 level count=5 | u8 fill grey per level (0,137,188,225,255)
//          | u8 flags (bit 0: outline unhinted -- CoreText's 1x snap is avoided by the ppem, so baud's
//            gate runs with its auto-hinter off; bit 1: phase 0, integral pen)
//          | u16 length + UTF-8 macOS version string (ProcessInfo.operatingSystemVersionString)
//   glyph: u32 codepoint | u16 w | u16 h | i16 pen_x | i16 pen_y | 6 masks of w*h u8, rows top-down:
//          mask 0 = smoothing off, masks 1..5 = smoothing on at levels 0..4.
// pen_x, pen_y: the glyph origin (pen on the baseline) in pixels from the mask's top-left corner,
// y down. The origin is an integer device pixel in CoreText and in the mask, so a baud mask drawn
// with Fit at phase 0 registers against mask 0 by the pen alone, no search.
// Rendered in an alpha-only CGBitmapContext: the fill grey picks CoreText's smoothing level exactly
// as an RGB fill of the same luminance does (checked byte-identical), and alpha holds coverage at
// full u8 precision. Subpixel positioning and quantisation are off; the pen is integral.
import CoreGraphics
import CoreText
import Foundation

let args = CommandLine.arguments
let fontURL = URL(fileURLWithPath: args[1]); let outDir = args[2]
let fontData = try! Data(contentsOf: fontURL)
let cgFont = CGFont(CGDataProvider(data: fontData as CFData)!)!
let greys: [UInt8] = [0, 137, 188, 225, 255]
// ppem: integer + 0.001 at 1x sizes (CoreText's vertical snap needs an exact integer ppem <= 18),
// integers from 19 up (no snap), through the cap on d at every level (n0 caps at 51.7 px).
let sizes: [Double] = [9.001, 10.001, 11.001, 12.001, 13.001, 14.001, 15.001, 16.001, 17.001, 18.001,
                       20, 22, 24, 26, 28, 32, 36, 48]
func fnv(_ d: Data) -> UInt32 { var h: UInt32 = 0x811c9dc5; for b in d { h = (h ^ UInt32(b)) &* 0x01000193 }; return h }

func render(_ g: CGGlyph, _ f: CTFont, grey: UInt8, smooth: Bool, W: Int, H: Int, pen: (Int, Int)) -> [UInt8] {
    let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W, space: nil,
                        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.alphaOnly.rawValue))!
    ctx.setShouldAntialias(true); ctx.setAllowsAntialiasing(true)
    ctx.setAllowsFontSmoothing(true); ctx.setShouldSmoothFonts(smooth)
    ctx.setAllowsFontSubpixelPositioning(false); ctx.setShouldSubpixelPositionFonts(false)
    ctx.setAllowsFontSubpixelQuantization(false); ctx.setShouldSubpixelQuantizeFonts(false)
    ctx.setFillColor(CGColor(gray: CGFloat(grey) / 255, alpha: 1)); ctx.textMatrix = .identity
    var gg = [g]; var p = [CGPoint(x: pen.0, y: H - pen.1)]
    CTFontDrawGlyphs(f, &gg, &p, 1, ctx)
    let b = ctx.data!.bindMemory(to: UInt8.self, capacity: W * H)
    return Array(UnsafeBufferPointer(start: b, count: W * H))
}

var total = 0
for px in sizes {
    let f = CTFontCreateWithGraphicsFont(cgFont, CGFloat(px), nil, nil)
    let W = Int(1.4 * px) + 16, H = Int(1.8 * px) + 16; let pen = (8, Int((1.3 * px).rounded(.up)) + 8)
    var body = Data(); var count = 0
    for cp in Array(0x21...0x7e) + [0x2580, 0x2584, 0x2588, 0x258C, 0x2590] {
        var u = [UniChar(cp)]; var g = [CGGlyph(0)]
        guard CTFontGetGlyphsForCharacters(f, &u, &g, 1), g[0] != 0 else { fatalError("no glyph for \(cp)") }
        var masks = [render(g[0], f, grey: 0, smooth: false, W: W, H: H, pen: pen)]
        for gv in greys { masks.append(render(g[0], f, grey: gv, smooth: true, W: W, H: H, pen: pen)) }
        var x0 = W, x1 = -1, y0 = H, y1 = -1
        for m in masks { for y in 0..<H { for x in 0..<W where m[y * W + x] != 0 { x0 = min(x0, x); x1 = max(x1, x); y0 = min(y0, y); y1 = max(y1, y) } } }
        precondition(x1 >= 0 && x0 > 0 && y0 > 0 && x1 < W - 1 && y1 < H - 1, "ink touches the canvas edge at \(px) cp \(cp)")
        let w = x1 - x0 + 1, h = y1 - y0 + 1
        func le<T: FixedWidthInteger>(_ v: T) { withUnsafeBytes(of: v.littleEndian) { body.append(contentsOf: $0) } }
        le(UInt32(cp)); le(UInt16(w)); le(UInt16(h)); le(Int16(pen.0 - x0)); le(Int16(pen.1 - y0))
        for m in masks { for y in y0...y1 { body.append(contentsOf: m[(y * W + x0)...(y * W + x1)]) } }
        count += 1
    }
    var file = Data("BDIL".utf8)
    func le<T: FixedWidthInteger>(_ v: T) { withUnsafeBytes(of: v.littleEndian) { file.append(contentsOf: $0) } }
    le(UInt16(1)); le(UInt16(count)); le(Float(px).bitPattern); le(UInt32(fontData.count)); le(fnv(fontData))
    file.append(5); file.append(contentsOf: greys); file.append(0b11)
    let osv = Data(ProcessInfo.processInfo.operatingSystemVersionString.utf8); le(UInt16(osv.count)); file.append(osv)
    file.append(body)
    let name = String(format: "ct-smooth-%@.bin", px == px.rounded() ? String(Int(px)) : String(format: "%.3f", px))
    try! file.write(to: URL(fileURLWithPath: outDir + "/" + name))
    total += file.count
    print(name, file.count, "bytes,", count, "glyphs")
}
print("total", total, "bytes")
