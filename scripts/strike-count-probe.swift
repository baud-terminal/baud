// What CoreText does with an sbix table of many strikes: whether it opens the
// font, whether it still draws a glyph's bitmap, and what a cold and a warm
// draw cost. Note J, docs/research/notes/j-colour-bitmap-limits.md.
// NOTHING IS DRAWN ON SCREEN: every glyph goes into an offscreen
// CGBitmapContext. One font a process, so a draw is cold the first time.
//
//   python3 scripts/strike-count-fonts.py OUT
//   env -u SDKROOT DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
//     /usr/bin/swiftc -O scripts/strike-count-probe.swift -o OUT/probe
//   OUT/probe OUT/ct-sbix-distinct-65535.ttf
//
// The devenv shell's DEVELOPER_DIR names a nix SDK with no Swift toolchain,
// hence the explicit one. Feed it the ct-* fonts: CGFont refuses the others.
// In them glyph 1 is a png in every strike, glyph 3 a flip to it, glyph 0
// holds nothing anywhere and glyph 9 is a dupe naming itself.
import CoreGraphics
import CoreText
import Foundation

func now() -> UInt64 { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }

let path = CommandLine.arguments[1]
let t0 = now()
guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)),
      let provider = CGDataProvider(data: data as CFData),
      let cg = CGFont(provider)
else {
    print("\(path): CGFont refused the file")
    exit(0)
}
let font = CTFontCreateWithGraphicsFont(cg, 20, nil, nil)
let opened = now() - t0

let side = 64
let space = CGColorSpace(name: CGColorSpace.sRGB)!
func context() -> CGContext {
    CGContext(data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side * 4,
              space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
}
func inked(_ c: CGContext) -> Int {
    let p = c.data!.bindMemory(to: UInt8.self, capacity: side * side * 4)
    var n = 0
    for i in 0..<(side * side) where p[i * 4 + 3] != 0 { n += 1 }
    return n
}

print("\(path): opened in \(opened / 1000) us")
for (name, g) in [("glyph 0, held nowhere", 0), ("glyph 9, a dupe loop", 9), ("glyph 1, a png", 1), ("glyph 3, a flip", 3)] {
    var glyph = CGGlyph(g)
    var at = CGPoint(x: 16, y: 16)
    let cold = context()
    let t = now()
    CTFontDrawGlyphs(font, &glyph, &at, 1, cold)
    let first = now() - t
    var times: [UInt64] = []
    for _ in 0..<7 {
        let c = context()
        let s = now()
        CTFontDrawGlyphs(font, &glyph, &at, 1, c)
        times.append(now() - s)
    }
    times.sort()
    print("  \(name): first draw \(first / 1000) us, then median \(times[3] / 1000) us; \(inked(cold)) pixels inked")
}
