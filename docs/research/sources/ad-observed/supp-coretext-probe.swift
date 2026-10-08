import Foundation
import CoreText
if let reg = ProcessInfo.processInfo.environment["REGISTER_FONT"] { var e: Unmanaged<CFError>?; print("register", reg, CTFontManagerRegisterFontsForURL(URL(fileURLWithPath: reg) as CFURL, .process, &e)) }
let fname = CommandLine.arguments[1]
let size = CGFloat(Double(CommandLine.arguments[2])!)
let base = CTFontCreateWithName(fname as CFString, size, nil)
print("base font:", CTFontCopyPostScriptName(base))
let text = try! String(contentsOfFile: CommandLine.arguments[3], encoding: .utf8)
for line in text.split(separator: "\n") {
  let f = line.split(separator: " ")
  let id = f[0]
  var s = ""
  for h in f.dropFirst() { s.unicodeScalars.append(Unicode.Scalar(UInt32(h, radix: 16)!)!) }
  let ns = s as NSString
  var cov: [String] = []
  for u in s.unicodeScalars {
    let us = String(u) as NSString
    var chars = [UniChar](repeating: 0, count: us.length)
    us.getCharacters(&chars, range: NSRange(location: 0, length: us.length))
    var g = [CGGlyph](repeating: 0, count: us.length)
    let ok = CTFontGetGlyphsForCharacters(base, chars, &g, us.length)
    let fb = CTFontCopyPostScriptName(CTFontCreateForString(base, us as CFString, CFRange(location: 0, length: us.length))) as String
    cov.append(String(format: "U+%04X base:%@ fallback:%@", u.value, ok ? "yes" : "no", fb))
  }
  print("== \(id) [\(f.dropFirst().joined(separator: " "))] utf16=\(ns.length)")
  for c in cov { print("   ", c) }
  let attr = NSAttributedString(string: s, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): base])
  let lineRef = CTLineCreateWithAttributedString(attr)
  let runs = CTLineGetGlyphRuns(lineRef) as! [CTRun]
  for r in runs {
    let n = CTRunGetGlyphCount(r)
    let attrs = CTRunGetAttributes(r) as NSDictionary
    let rf = attrs[kCTFontAttributeName as String] as! CTFont
    let cg = CTFontCopyGraphicsFont(rf, nil)
    var gl = [CGGlyph](repeating: 0, count: n); CTRunGetGlyphs(r, CFRange(location: 0, length: 0), &gl)
    var idx = [CFIndex](repeating: 0, count: n); CTRunGetStringIndices(r, CFRange(location: 0, length: 0), &idx)
    var pos = [CGPoint](repeating: .zero, count: n); CTRunGetPositions(r, CFRange(location: 0, length: 0), &pos)
    var adv = [CGSize](repeating: .zero, count: n); CTRunGetAdvances(r, CFRange(location: 0, length: 0), &adv)
    let sr = CTRunGetStringRange(r)
    var desc: [String] = []
    for i in 0..<n {
      let gname = (cg.name(for: gl[i]) as String?) ?? "?"
      desc.append(String(format: "g%d(%@)@i%d x=%.2f y=%.2f adv=%.2f", gl[i], gname, idx[i], pos[i].x, pos[i].y, adv[i].width))
    }
    print("   run font=\(CTFontCopyPostScriptName(rf)) str=\(sr.location)+\(sr.length): " + desc.joined(separator: "; "))
  }
}
