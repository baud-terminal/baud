// For a font (file path, or name:<PostScript name>), render glyphs at integer px and px+0.001
// at 1x grayscale, no smoothing; report per glyph the top-ink height (sum of ink above the
// row where the glyph's top half starts, i.e. the ink-weighted top) difference.
import CoreGraphics
import CoreText
import Foundation
let spec = CommandLine.arguments[1]
func mk(_ px: Double) -> CTFont {
    if spec.hasPrefix("name:") { return CTFontCreateWithName(String(spec.dropFirst(5)) as CFString, CGFloat(px), nil) }
    let cg = CGFont(CGDataProvider(url: URL(fileURLWithPath: spec) as CFURL)!)!
    return CTFontCreateWithGraphicsFont(cg, CGFloat(px), nil, nil) }
let W = 60, H = 70, base = 50
let SC = Double(ProcessInfo.processInfo.environment["SCALE"] ?? "1")!
func ink(_ ch: Character, _ px: Double) -> [Double] {
    let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W,
        space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!
    ctx.setFillColor(gray: 1, alpha: 1); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
    ctx.setShouldSmoothFonts(false); ctx.setFillColor(gray: 0, alpha: 1)
    ctx.setShouldSubpixelPositionFonts(false); ctx.setShouldSubpixelQuantizeFonts(false)
    ctx.scaleBy(x: CGFloat(SC), y: CGFloat(SC)); let f = mk(px / SC); var c = Array(String(ch).utf16); var g = [CGGlyph](repeating: 0, count: 1)
    _ = CTFontGetGlyphsForCharacters(f, &c, &g, 1)
    var p = [CGPoint(x: 10 / CGFloat(SC), y: CGFloat(H - base) / CGFloat(SC))]; CTFontDrawGlyphs(f, g, &p, 1, ctx)
    let b = ctx.data!.bindMemory(to: UInt8.self, capacity: W * H)
    if ProcessInfo.processInfo.environment["COLS"] != nil { return (0..<W).map { x in (0..<H).reduce(0.0) { $0 + 1 - Double(b[$1 * W + x]) / 255 } } }
    return (0..<H).map { y in (0..<W).reduce(0.0) { $0 + 1 - Double(b[y * W + $1]) / 255 } } }
// ink-weighted height of the top 25% of ink mass and bottom 25%: robust edge proxies
func q(_ r: [Double]) -> (Double, Double) {
    let tot = r.reduce(0,+); var acc = 0.0; var top = 0.0; var bot = 0.0
    for y in 0..<H { acc += r[y]; if acc >= 0.1 * tot { top = Double(base - y); break } }
    acc = 0; for y in stride(from: H-1, through: 0, by: -1) { acc += r[y]; if acc >= 0.1 * tot { bot = Double(base - y - 1); break } }
    return (top, bot) }
let glyphs = Array("xzHonlTkEgp")
for px in [10.0, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38] {
    var s = String(format: "%4.0f:", px); var moved = 0
    for ch in glyphs {
        let a = ink(ch, px), b = ink(ch, px + 0.001)
        let ta = a.reduce(0,+), tb = b.reduce(0,+)
        let d = zip(a, b).reduce(0.0) { $0 + abs($1.0 / ta - $1.1 / tb) }
        let tot = 1.0
        if d / tot > 0.02 { moved += 1; s += " \(ch)" }
    }
    print(s, " moved \(moved)/\(glyphs.count)")
}
