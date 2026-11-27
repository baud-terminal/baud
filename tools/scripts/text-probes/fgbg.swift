import CoreGraphics
import CoreText
import Foundation
// Check 1: in an sRGB RGBA context (blend proven linear in stored values), does dilation
// depend on fg only, or on bg too? Stem gain = sum over a row of recovered coverage
// ((v-bg)/(fg-bg) on the most informative channel, or alpha on transparent bg) with
// smoothing on, minus the same with smoothing off. Menlo 'l' at 13 and 20 px.
let cs = CGColorSpace(name: CGColorSpace.sRGB)!
func stem(px: CGFloat, fg: (CGFloat,CGFloat,CGFloat), bg: (CGFloat,CGFloat,CGFloat,CGFloat), smooth: Bool) -> Double {
    let w = Int(px*3)+16, h = Int(px*2)+16
    var buf = [UInt8](repeating: 0, count: w*h*4)
    let ctx = CGContext(data: &buf, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w*4, space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    if bg.3 > 0 { ctx.setFillColor(red: bg.0, green: bg.1, blue: bg.2, alpha: bg.3); ctx.fill(CGRect(x:0,y:0,width:w,height:h)) }
    ctx.setAllowsFontSmoothing(true); ctx.setShouldSmoothFonts(smooth); ctx.setShouldAntialias(true)
    ctx.setAllowsFontSubpixelQuantization(false); ctx.setShouldSubpixelQuantizeFonts(false)
    ctx.setAllowsFontSubpixelPositioning(true); ctx.setShouldSubpixelPositionFonts(true)
    ctx.setFillColor(red: fg.0, green: fg.1, blue: fg.2, alpha: 1)
    let f = CTFontCreateWithName("Menlo-Regular" as CFString, px, nil)
    var ch: [UniChar] = [108]; var g = [CGGlyph](repeating: 0, count: 1)
    CTFontGetGlyphsForCharacters(f, &ch, &g, 1)
    var p = [CGPoint(x: px*0.5+0.3, y: 4)]
    CTFontDrawGlyphs(f, &g, &p, 1, ctx)
    let row = h - 1 - Int(4 + px*0.3)
    // choose channel with largest |fg-bg|
    let fgs = [fg.0, fg.1, fg.2], bgs = [bg.0, bg.1, bg.2]
    var s = 0.0
    for x in 0..<w {
        let i = (row*w+x)*4
        if bg.3 == 0 { s += Double(buf[i+3])/255; continue }
        var best = 0; var bd = 0.0
        for c in 0..<3 { let d = abs(Double(fgs[c]-bgs[c])); if d > bd { bd = d; best = c } }
        s += (Double(buf[i+best])/255 - Double(bgs[best])) / (Double(fgs[best]) - Double(bgs[best]))
    }
    return s
}
func gain(_ px: CGFloat, _ fg: (CGFloat,CGFloat,CGFloat), _ bg: (CGFloat,CGFloat,CGFloat,CGFloat)) -> Double {
    stem(px: px, fg: fg, bg: bg, smooth: true) - stem(px: px, fg: fg, bg: bg, smooth: false)
}
for px in [13.0, 20.0] as [CGFloat] {
  print(String(format: "== %.0f px (alpha-only levels: 13px .149/.220/.282/.345/.408; 20px .235/.337/.431/.533/.604)", px))
  print("  sweep fg gray, bg transparent:", [0.0,0.25,0.5,0.75,1.0].map { String(format: "%.2f:%.3f", $0, gain(px, ($0,$0,$0), (0,0,0,0))) }.joined(separator: " "))
  print("  sweep fg gray, bg black:      ", [0.25,0.5,0.75,1.0].map { String(format: "%.2f:%.3f", $0, gain(px, ($0,$0,$0), (0,0,0,1))) }.joined(separator: " "))
  print("  sweep fg gray, bg white:      ", [0.0,0.25,0.5,0.75].map { String(format: "%.2f:%.3f", $0, gain(px, ($0,$0,$0), (1,1,1,1))) }.joined(separator: " "))
  print("  sweep bg gray, fg white:      ", [0.0,0.25,0.5,0.75].map { String(format: "%.2f:%.3f", $0, gain(px, (1,1,1), ($0,$0,$0,1))) }.joined(separator: " "))
  print("  sweep bg gray, fg black:      ", [0.25,0.5,0.75,1.0].map { String(format: "%.2f:%.3f", $0, gain(px, (0,0,0), ($0,$0,$0,1))) }.joined(separator: " "))
  print("  fg 0.5 over bg 0/0.25/0.75/1: ", [0.0,0.25,0.75,1.0].map { String(format: "%.2f:%.3f", $0, gain(px, (0.5,0.5,0.5), ($0,$0,$0,1))) }.joined(separator: " "))
  // check 2: channel weights; transparent bg so only fg matters
  let cols: [(String,(CGFloat,CGFloat,CGFloat))] = [("R1",(1,0,0)),("G1",(0,1,0)),("B1",(0,0,1)),("R.5",(0.5,0,0)),("G.5",(0,0.5,0)),("Y(1,1,0)",(1,1,0)),("C(0,1,1)",(0,1,1)),("gray.39",(0.39,0.39,0.39)),("gray.38",(0.38,0.38,0.38)),("G.7",(0,0.7,0)),("G.75",(0,0.75,0)),("G.8",(0,0.8,0)),("G.85",(0,0.85,0)),("G.9",(0,0.9,0)),("G.95",(0,0.95,0))]
  print("  colours, transparent bg:      ", cols.map { String(format: "%@:%.3f", $0.0, gain(px, $0.1, (0,0,0,0))) }.joined(separator: " "))
}
