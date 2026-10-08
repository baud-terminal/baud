import Foundation
import CoreText
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
// Renders, for each case: (A) CTLineDraw of the string in the base font; (B) each run's glyphs drawn with CTFontDrawGlyphs at the run's own positions.
let fname = CommandLine.arguments[1]
let size = CGFloat(Double(CommandLine.arguments[2])!)
let out = CommandLine.arguments[4]
let base = CTFontCreateWithName(fname as CFString, size, nil)
let text = try! String(contentsOfFile: CommandLine.arguments[3], encoding: .utf8)
let lines = text.split(separator: "\n")
let rowH = size * 1.6
let W = Int(size * 30), H = Int(rowH * CGFloat(lines.count + 1))
let cs = CGColorSpaceCreateDeviceRGB()
let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 0, space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1)); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
let label = CTFontCreateWithName("Menlo-Regular" as CFString, size * 0.45, nil)
func draw(_ s: String, _ f: CTFont, _ x: CGFloat, _ y: CGFloat) {
  let a = NSAttributedString(string: s, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): f, NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String): true])
  let l = CTLineCreateWithAttributedString(a)
  ctx.textPosition = CGPoint(x: x, y: y); CTLineDraw(l, ctx)
}
ctx.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
for (i, line) in lines.enumerated() {
  let f = line.split(separator: " ")
  var s = ""
  for h in f.dropFirst() { s.unicodeScalars.append(Unicode.Scalar(UInt32(h, radix: 16)!)!) }
  let y = CGFloat(H) - rowH * CGFloat(i + 1)
  draw(String(line), label, 4, y + size * 0.2)
  // A: CTLineDraw
  ctx.setStrokeColor(CGColor(red: 0.8, green: 0.2, blue: 0.2, alpha: 1)); ctx.setLineWidth(1)
  let ax = size * 8
  ctx.stroke(CGRect(x: ax, y: y - size*0.3, width: 0.5, height: size * 1.3))
  draw(s, base, ax, y)
  // B: per-run CTFontDrawGlyphs using advances accumulated from x0 (Terminal-like: glyphs + advances)
  let a = NSAttributedString(string: s, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): base])
  let l = CTLineCreateWithAttributedString(a)
  var x = size * 16
  ctx.stroke(CGRect(x: x, y: y - size*0.3, width: 0.5, height: size * 1.3))
  for r in CTLineGetGlyphRuns(l) as! [CTRun] {
    let n = CTRunGetGlyphCount(r)
    let rf = (CTRunGetAttributes(r) as NSDictionary)[kCTFontAttributeName as String] as! CTFont
    var gl = [CGGlyph](repeating: 0, count: n); CTRunGetGlyphs(r, CFRange(location: 0, length: 0), &gl)
    var adv = [CGSize](repeating: .zero, count: n); CTRunGetAdvances(r, CFRange(location: 0, length: 0), &adv)
    var pts: [CGPoint] = []
    for k in 0..<n { pts.append(CGPoint(x: x, y: y)); x += adv[k].width }
    ctx.textMatrix = .identity; ctx.textPosition = .zero; CTFontDrawGlyphs(rf, gl, pts, n, ctx)
  }
}
let img = ctx.makeImage()!
let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(dest, img, nil); CGImageDestinationFinalize(dest)
print("wrote", out, W, H)
