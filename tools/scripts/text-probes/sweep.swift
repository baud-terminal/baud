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
let name = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "Menlo-Regular"
let base = CTFontCreateWithName(name as CFString, 12, nil)
print("font:", CTFontCopyPostScriptName(base))
for px in [13.0, 20.0] as [CGFloat] {
  let off = stemw(base, px: px, fill: 1, smooth: false, scale: 1)
  var s = String(format: "%5.1fpx off=%.3f dil(L):", px, off)
  for i in 0...20 { let L = CGFloat(i)/20; s += String(format: " %.2f:%.3f", L, stemw(base, px: px, fill: L, smooth: true, scale: 1) - off) }
  print(s)
}
// scale test: 13pt at CTM scale 2 vs 26px at scale 1, same device pixels
for L in [0.0, 1.0] as [CGFloat] {
  let a = stemw(base, px: 13, fill: L, smooth: true, scale: 2) - stemw(base, px: 13, fill: L, smooth: false, scale: 2)
  let b = stemw(base, px: 26, fill: L, smooth: true, scale: 1) - stemw(base, px: 26, fill: L, smooth: false, scale: 1)
  print(String(format: "L=%.0f: 13pt@2x dil=%.3f devpx ; 26px@1x dil=%.3f devpx", L, a, b))
}
