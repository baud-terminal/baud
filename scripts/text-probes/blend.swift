import CoreGraphics
import CoreText
import Foundation
// Is CG's text AA blend linear in encoded values? Compare coverage (alpha-only mask)
// with the pixel of the same glyph drawn black on white / white on black in an sRGB RGBA context.
let W = 64, H = 64
func draw(_ ctx: CGContext, fill: CGFloat, smooth: Bool) {
    ctx.setAllowsFontSmoothing(true); ctx.setShouldSmoothFonts(smooth); ctx.setShouldAntialias(true)
    ctx.setAllowsFontSubpixelQuantization(false); ctx.setShouldSubpixelQuantizeFonts(false)
    ctx.setAllowsFontSubpixelPositioning(true); ctx.setShouldSubpixelPositionFonts(true)
    ctx.setFillColor(red: fill, green: fill, blue: fill, alpha: 1)
    let f = CTFontCreateWithName("Menlo-Regular" as CFString, 40, nil)
    var ch: [UniChar] = [UniChar(111)]; var g = [CGGlyph](repeating: 0, count: 1)
    CTFontGetGlyphsForCharacters(f, &ch, &g, 1)
    var p = [CGPoint(x: 8.3, y: 12.4)]
    CTFontDrawGlyphs(f, &g, &p, 1, ctx)
}
var mask = [UInt8](repeating: 0, count: W*H)
let mctx = CGContext(data: &mask, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.alphaOnly.rawValue)!
draw(mctx, fill: 0, smooth: false)
for (name, cs) in [("sRGB", CGColorSpace(name: CGColorSpace.sRGB)!), ("linearSRGB", CGColorSpace(name: CGColorSpace.linearSRGB)!)] {
 for fg in [0.0, 1.0] as [CGFloat] {
  var px = [UInt8](repeating: 0, count: W*H*4)
  let c = CGContext(data: &px, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W*4, space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
  c.setFillColor(red: 1-fg, green: 1-fg, blue: 1-fg, alpha: 1); c.fill(CGRect(x:0,y:0,width:W,height:H))
  draw(c, fill: fg, smooth: false)
  // collect (coverage, pixel) pairs for partial coverage
  var buckets = [Int: [Double]]()
  for i in 0..<(W*H) { let a = Int(mask[i]); if a > 0 && a < 255 { let v = Double(px[i*4]); let t = fg == 0 ? (255 - v) : v; buckets[a/32, default: []].append(t/255) } }
  var s = "\(name) fg=\(fg): coverage-bucket -> mean fg-weight:"
  for k in buckets.keys.sorted() { let a = buckets[k]!; s += String(format: " %.2f:%.2f", (Double(k)+0.5)*32/255, a.reduce(0,+)/Double(a.count)) }
  print(s)
 }
}
