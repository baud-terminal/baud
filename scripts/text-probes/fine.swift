import CoreGraphics
import CoreText
import Foundation
func stemw(_ font: CTFont, px: CGFloat, fill: CGFloat, smooth: Bool, scale: CGFloat, char: Character = "l") -> Double {
    let w = Int(px*scale*3)+16, h = Int(px*scale*2)+16
    var buf = [UInt8](repeating: 0, count: w*h)
    let ctx = CGContext(data: &buf, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.alphaOnly.rawValue)!
    ctx.setAllowsFontSmoothing(true); ctx.setShouldSmoothFonts(smooth)
    ctx.setShouldAntialias(true)
    ctx.setAllowsFontSubpixelQuantization(false); ctx.setShouldSubpixelQuantizeFonts(false)
    ctx.setAllowsFontSubpixelPositioning(true); ctx.setShouldSubpixelPositionFonts(true)
    ctx.scaleBy(x: scale, y: scale)
    ctx.setFillColor(gray: fill, alpha: 1)
    let f = CTFontCreateCopyWithAttributes(font, px, nil, nil)
    var ch: [UniChar] = [UniChar(char.asciiValue!)]
    var g = [CGGlyph](repeating: 0, count: 1)
    CTFontGetGlyphsForCharacters(f, &ch, &g, 1)
    var p = [CGPoint(x: px*0.5+0.3, y: 4)]
    CTFontDrawGlyphs(f, &g, &p, 1, ctx)
    let row = h - 1 - Int((4 + px*0.3)*scale)
    var sum = 0.0
    for x in 0..<w { sum += Double(buf[row*w+x])/255 }
    return sum
}
let base = CTFontCreateWithName("Menlo-Regular" as CFString, 12, nil)
let off20 = stemw(base, px: 20, fill: 1, smooth: false, scale: 1)
var prev = -1.0; var s = "breaks at 20px:"
for i in 0...200 { let L = CGFloat(i)/200; let d = stemw(base, px: 20, fill: L, smooth: true, scale: 1) - off20
  if abs(d - prev) > 0.001 { s += String(format: " [L>=%.3f -> %.3f]", L, d); prev = d } }
print(s)
// dilation per level vs size
let reps: [CGFloat] = [0.0, 0.5, 0.75, 0.9, 1.0]
for px in [6.0,8,10,12,14,16,18,20,22,24,26,28,30,34,40] as [CGFloat] {
  let off = stemw(base, px: px, fill: 1, smooth: false, scale: 1)
  var l = String(format: "%4.0fpx", px)
  for L in reps { let d = stemw(base, px: px, fill: L, smooth: true, scale: 1) - off; l += String(format: "  L%.2f %.3f (%.5f/ppem)", L, d, d/Double(px)) }
  print(l)
}
