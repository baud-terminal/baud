// CoreText flat-edge positions at 1x, grayscale, vs design, per ppem.
// zones2 <font> <smooth 0|1> <yfrac> <upm-scale-of-design:1000>
import CoreGraphics
import CoreText
import Foundation
let path = CommandLine.arguments[1]
let smooth = CommandLine.arguments[2] == "1"
let yfrac = Double(CommandLine.arguments[3])!
let cg = CGFont(CGDataProvider(url: URL(fileURLWithPath: path) as CFURL)!)!
let upm = Double(cg.unitsPerEm)
let W = 80, H = 90, base = 60, ox = 20
func cov(_ ch: Character, _ px: Double) -> [[Double]] {
    let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W,
        space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!
    ctx.setFillColor(gray: 1, alpha: 1); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
    ctx.setShouldAntialias(true); ctx.setAllowsFontSmoothing(true); ctx.setShouldSmoothFonts(smooth)
    ctx.setAllowsFontSubpixelPositioning(false); ctx.setShouldSubpixelPositionFonts(false)
    ctx.setAllowsFontSubpixelQuantization(false); ctx.setShouldSubpixelQuantizeFonts(false)
    ctx.setFillColor(gray: 0, alpha: 1)
    let font = CTFontCreateWithGraphicsFont(cg, CGFloat(px), nil, nil)
    var c = Array(String(ch).utf16); var g = [CGGlyph](repeating: 0, count: 1)
    _ = CTFontGetGlyphsForCharacters(font, &c, &g, 1)
    var p = [CGPoint(x: CGFloat(ox), y: CGFloat(H - base) + CGFloat(yfrac))]
    CTFontDrawGlyphs(font, g, &p, 1, ctx)
    let b = ctx.data!.bindMemory(to: UInt8.self, capacity: W * H)
    return (0..<H).map { y in (0..<W).map { x in 1 - Double(b[y * W + x]) / 255 } }
}
// Row profile over design x-range [dx0,dx1] (font units), normalised to the band width.
func profile(_ m: [[Double]], _ px: Double, _ dx0: Double, _ dx1: Double) -> [Double] {
    let x0 = Double(ox) + dx0 * px / upm, x1 = Double(ox) + dx1 * px / upm
    // the one column with the most ink at the band's centre row range: a stem
    // narrower than a pixel still gives a column whose profile, normalised by
    // its body value, is the edge's coverage.
    var c0 = Int(floor(x0)), c1 = max(c0 + 1, Int(ceil(x1)))
    if x1 - x0 > 3 { c0 = Int(ceil(x0)); c1 = Int(floor(x1)) } // wide band: inner columns only
    else {
        var best = c0; var bv = -1.0
        for c in c0..<c1 { let v = m.reduce(0.0) { $0 + $1[c] }; if v > bv { bv = v; best = c } }
        c0 = best; c1 = best + 1 }
    return m.map { r in (c0..<c1).reduce(0.0) { $0 + r[$1] } }
}
func norm(_ r: [Double], _ body: Int) -> [Double] { let mx = r[body]; return r.map { $0 / mx } }
// Total ink above a y threshold row: the position of the top edge of a solid region
// whose top lies within rows [lo, hi] (from top). Edge (px above baseline) = rows fully inked
// counted from 'bodyRow' upward plus partial.
func topEdge(_ r: [Double], below: Int) -> Double { // below: a row inside the solid body
    var s = 0.0; var y = below; while y >= 0 && r[y] > 0.005 { s += min(1, r[y]); y -= 1 }
    return Double(base - below - 1) + s }
func bottomEdge(_ r: [Double], above: Int) -> Double {
    var s = 0.0; var y = above; while y < H && r[y] > 0.005 { s += min(1, r[y]); y += 1 }
    return Double(base - above) - s }
func row(_ yUp: Double) -> Int { base - 1 - Int(floor(yUp)) } // row containing height yUp above baseline
print("px | xh d/ct | cap d/ct | base0 ct | desc d/ct | bar_lo d/ct | bar_hi d/ct | uOver d/ct")
var sizes: [Double] = []; if let e = ProcessInfo.processInfo.environment["SIZES"] { sizes = e.split(separator: ",").map { Double($0)! } } else { for i in stride(from: 90, through: 240, by: 5) { sizes.append(Double(i)/10) } }
for px in sizes {
    let s = px / upm
    let ru = row(275 * s), rh = row(200 * s), rb = row(377 * s), rp = row(270 * s)
    let u = norm(profile(cov("u", px), px, 90, 180), ru)
    let xh = topEdge(u, below: ru)
    let hm = cov("H", px)
    let hs = norm(profile(hm, px, 93, 183), rh)
    let cap = topEdge(hs, below: rh)
    let b0 = bottomEdge(hs, above: rh)
    let bar = norm(profile(hm, px, 230, 370), rb)
    let barHi = topEdge(bar, below: rb), barLo = bottomEdge(bar, above: rb)
    let pp = norm(profile(cov("p", px), px, 92, 182), rp)
    let desc = bottomEdge(pp, above: rp)
    let uo = 0.0
    print(String(format: "%.1f | %.3f %.3f | %.3f %.3f | %.3f | %.3f %.3f | %.3f %.3f | %.3f %.3f | %.3f %.3f", px, 550*s, xh, 730*s, cap, b0, -180*s, desc, 336*s, barLo, 418*s, barHi, -10*s, uo))
}
