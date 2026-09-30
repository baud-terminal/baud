import CoreGraphics
import CoreText
import Foundation
import AppKit
// (1) Edge profile of Menlo 'l' row at 20px, smoothing off vs on (black and white fill), 4 phases.
func row(px: CGFloat, fill: CGFloat, smooth: Bool, phase: CGFloat) -> [Double] {
    let w = Int(px*2)+16, h = Int(px*2)+16
    var buf = [UInt8](repeating: 0, count: w*h)
    let ctx = CGContext(data: &buf, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.alphaOnly.rawValue)!
    ctx.setAllowsFontSmoothing(true); ctx.setShouldSmoothFonts(smooth); ctx.setShouldAntialias(true)
    ctx.setAllowsFontSubpixelQuantization(false); ctx.setShouldSubpixelQuantizeFonts(false)
    ctx.setAllowsFontSubpixelPositioning(true); ctx.setShouldSubpixelPositionFonts(true)
    ctx.setFillColor(gray: fill, alpha: 1)
    let f = CTFontCreateWithName("Menlo-Regular" as CFString, px, nil)
    var ch: [UniChar] = [108]; var g = [CGGlyph](repeating: 0, count: 1)
    CTFontGetGlyphsForCharacters(f, &ch, &g, 1)
    var p = [CGPoint(x: 4 + phase, y: 4)]
    CTFontDrawGlyphs(f, &g, &p, 1, ctx)
    let r = h - 1 - Int(4 + px*0.3)
    return (0..<w).map { Double(buf[r*w+$0])/255 }
}
func stats(_ v: [Double]) -> (Double, Double) { let s = v.reduce(0,+); var m = 0.0; for (i,x) in v.enumerated() { m += Double(i)*x }; return (s, m/s) }
for phase in [0.0, 0.25, 0.5, 0.75] as [CGFloat] {
  let off = row(px: 20, fill: 1, smooth: false, phase: phase)
  let b = row(px: 20, fill: 0, smooth: true, phase: phase), wt = row(px: 20, fill: 1, smooth: true, phase: phase)
  let nz = off.indices.filter { off[$0] > 0 || b[$0] > 0 || wt[$0] > 0 }
  let lo = max(0, (nz.first ?? 0)), hi = min(off.count-1, (nz.last ?? 0))
  func fmt(_ v:[Double]) -> String { (lo...hi).map { String(format: "%.3f", v[$0]) }.joined(separator: " ") }
  let (so, co) = stats(off), (sb, cb) = stats(b), (sw, cw) = stats(wt)
  print(String(format: "phase %.2f  cols %d..%d", phase, lo, hi))
  print("  off  : " + fmt(off) + String(format: "  sum %.3f centroid %.3f", so, co))
  print("  L0   : " + fmt(b) + String(format: "  sum %.3f centroid %.3f", sb, cb))
  print("  L1   : " + fmt(wt) + String(format: "  sum %.3f centroid %.3f", sw, cw))
}
// (2) AppKit default: NSAttributedString into NSBitmapImageRep, no explicit smoothing call.
func appkit(fg: NSColor, bg: NSColor, px: CGFloat) -> Double {
    let w = Int(px*2)+16, h = Int(px*2)+16
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: w*4, bitsPerPixel: 32)!.retagging(with: .sRGB)!
    NSGraphicsContext.saveGraphicsState()
    let gc = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = gc
    bg.setFill(); NSRect(x: 0, y: 0, width: w, height: h).fill()
    let s = NSAttributedString(string: "l", attributes: [.font: NSFont(name: "Menlo-Regular", size: px)!, .foregroundColor: fg])
    s.draw(at: NSPoint(x: px*0.5+0.3, y: 4 - (NSFont(name: "Menlo-Regular", size: px)!.descender)))
    gc.flushGraphics()
    NSGraphicsContext.restoreGraphicsState()
    let data = rep.bitmapData!
    let f = fg.usingColorSpace(.sRGB)!.redComponent, b = bg.usingColorSpace(.sRGB)!.redComponent
    // find row: middle of stem; scan rows, use one at 40% height of ink region
    var sums = [Double]()
    for r in 0..<h { var sum = 0.0; for x in 0..<w { let v = Double(data[(r*w+x)*4])/255; sum += (v - Double(b))/(Double(f)-Double(b)) }; sums.append(sum) }
    let ink = sums.indices.filter { sums[$0] > 0.5 }
    guard let a = ink.first, let z = ink.last else { return -1 }
    let mid = Array(sums[(a + (z-a)/3)...(a + 2*(z-a)/3)]).sorted()
    return mid[mid.count/2]
}
_ = NSApplication.shared
for px in [13.0, 20.0] as [CGFloat] {
  print(String(format: "AppKit %.0fpx  black-on-white stem %.3f   white-on-black stem %.3f   (CG alpha-only: off %@, n0 +%@, n4 +%@)", px,
    appkit(fg: .black, bg: .white, px: px), appkit(fg: .white, bg: .black, px: px), px == 13 ? "1.169" : "1.796", px == 13 ? ".149" : ".235", px == 13 ? ".408" : ".604"))
}
// reference: CG direct, same median-of-middle-third row statistic, opaque sRGB RGBA, smoothing off / on
func cgref(fill: CGFloat, px: CGFloat, smooth: Bool) -> Double {
    let w = Int(px*2)+16, h = Int(px*2)+16
    var buf = [UInt8](repeating: 0, count: w*h*4)
    let ctx = CGContext(data: &buf, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w*4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.setFillColor(red: 1-fill, green: 1-fill, blue: 1-fill, alpha: 1); ctx.fill(CGRect(x:0,y:0,width:w,height:h))
    ctx.setAllowsFontSmoothing(true); ctx.setShouldSmoothFonts(smooth); ctx.setShouldAntialias(true)
    ctx.setFillColor(red: fill, green: fill, blue: fill, alpha: 1)
    let f = CTFontCreateWithName("Menlo-Regular" as CFString, px, nil)
    var ch: [UniChar] = [108]; var g = [CGGlyph](repeating: 0, count: 1)
    CTFontGetGlyphsForCharacters(f, &ch, &g, 1)
    var p = [CGPoint(x: px*0.5+0.3, y: 6)]
    CTFontDrawGlyphs(f, &g, &p, 1, ctx)
    var sums = [Double]()
    for r in 0..<h { var sum = 0.0; for x in 0..<w { let v = Double(buf[(r*w+x)*4])/255; sum += (v - Double(1-fill))/(Double(fill)-Double(1-fill)) }; sums.append(sum) }
    let ink = sums.indices.filter { sums[$0] > 0.5 }
    let a = ink.first!, z = ink.last!
    let mid = Array(sums[(a + (z-a)/3)...(a + 2*(z-a)/3)]).sorted()
    return mid[mid.count/2]
}
for px in [13.0, 20.0] as [CGFloat] {
  print(String(format: "CG ref %.0fpx  BoW off %.3f on %.3f | WoB off %.3f on %.3f", px, cgref(fill: 0, px: px, smooth: false), cgref(fill: 0, px: px, smooth: true), cgref(fill: 1, px: px, smooth: false), cgref(fill: 1, px: px, smooth: true)))
}
