// smooth-cubics: CoreGraphics' fill of cubic paths at flatness 1, for baud's smoothing path.
//
//   smooth-cubics <out.bin>
//
// JetBrains Mono's outlines are quadratics, so the CoreText fixtures never reach the cubic half of
// the flattening rule baud reproduces (halve a cubic while 3/4 of the max-norm of its second
// differences exceeds 0.2 device px). These are cubic paths -- circles of four arcs and closed
// blobs of random cubics, at fractional positions, and blobs left open for the fill to close -- filled by CoreGraphics at flatness 1, which
// is how CoreText fills a glyph (`scratchpad/flatten3`: CTFontDrawGlyphs == a fill at flatness 1,
// 11-128 px). Deterministic: a fixed-seed generator, no font.
//
// File, little-endian: "BCUB" | u16 version=1 | u16 shape count
//   shape: u16 w | u16 h | u16 op count | ops | w*h u8 coverage, rows top-down
//   op:    u8 kind (0 move, 1 line, 3 cubic, 4 close) | f32 x,y per point (1, 1, 3, 0 points)
// Coordinates are device pixels, y down from the mask's top-left; the fill is non-zero winding.
import CoreGraphics
import Foundation

var seed: UInt64 = 0x9E37_79B9_7F4A_7C15
func rnd() -> Double { seed = seed &* 6364136223846793005 &+ 1442695040888963407; return Double(seed >> 11) / Double(1 << 53) }

enum Op { case m(Double, Double), l(Double, Double), c(Double, Double, Double, Double, Double, Double), z }
var shapes: [(Int, Int, [Op])] = []
// Circles: four arcs with the usual kappa, radius 1.5-24 px, centre at a random fraction.
for r in [1.5, 2.0, 3.0, 4.5, 6.0, 8.0, 11.0, 16.0, 24.0] {
    let k = 0.5522847498 * r, W = Int(2 * r) + 6, H = W
    let cx = r + 3 + rnd(), cy = r + 3 + rnd()
    shapes.append((W, H, [.m(cx + r, cy), .c(cx + r, cy + k, cx + k, cy + r, cx, cy + r),
                          .c(cx - k, cy + r, cx - r, cy + k, cx - r, cy),
                          .c(cx - r, cy - k, cx - k, cy - r, cx, cy - r),
                          .c(cx + k, cy - r, cx + r, cy - k, cx + r, cy), .z]))
}
// Blobs: three to six cubics through points on a jittered circle, control points random nearby.
for _ in 0..<30 {
    let size = 6 + 26 * rnd(), W = Int(size) + 8, H = W, n = 3 + Int(4 * rnd())
    let cx = Double(W) / 2, cy = Double(H) / 2
    var pts: [(Double, Double)] = []
    for i in 0..<n { let a = 2 * Double.pi * (Double(i) + 0.6 * rnd()) / Double(n), rr = size / 2 * (0.5 + 0.5 * rnd())
        pts.append((cx + rr * cos(a), cy + rr * sin(a))) }
    func jit(_ p: (Double, Double)) -> (Double, Double) { (p.0 + size * 0.35 * (rnd() - 0.5), p.1 + size * 0.35 * (rnd() - 0.5)) }
    var ops: [Op] = [.m(pts[0].0, pts[0].1)]
    for i in 0..<n { let a = pts[i], b = pts[(i + 1) % n]
        let c1 = jit((a.0 + (b.0 - a.0) / 3, a.1 + (b.1 - a.1) / 3)), c2 = jit((a.0 + 2 * (b.0 - a.0) / 3, a.1 + 2 * (b.1 - a.1) / 3))
        ops.append(.c(min(max(c1.0, 1), Double(W) - 1), min(max(c1.1, 1), Double(H) - 1),
                      min(max(c2.0, 1), Double(W) - 1), min(max(c2.1, 1), Double(H) - 1), b.0, b.1)) }
    ops.append(.z)
    shapes.append((W, H, ops))
}
// Open blobs: the last cubic dropped, so the contour ends away from its start and the fill's own
// closing line -- not a segment in the path -- finishes it.
for (W, H, ops) in Array(shapes.suffix(6)) {
    var o = ops; o.removeLast(2); o.append(.z)
    shapes.append((W, H, o))
}

var out = Data("BCUB".utf8)
func le<T: FixedWidthInteger>(_ v: T) { withUnsafeBytes(of: v.littleEndian) { out.append(contentsOf: $0) } }
func lef(_ v: Double) { le(Float(v).bitPattern) }
le(UInt16(1)); le(UInt16(shapes.count))
for (W, H, ops) in shapes {
    let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W, space: nil,
                        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.alphaOnly.rawValue))!
    ctx.setShouldAntialias(true); ctx.setFlatness(1); ctx.setFillColor(CGColor(gray: 0, alpha: 1))
    // y down in the file, y up in CoreGraphics: flip each point.
    let p = CGMutablePath(); let f = { (y: Double) in CGFloat(Double(H) - y) }
    for o in ops { switch o {
        case let .m(x, y): p.move(to: CGPoint(x: x, y: f(y)))
        case let .l(x, y): p.addLine(to: CGPoint(x: x, y: f(y)))
        case let .c(a, b, c, d, x, y): p.addCurve(to: CGPoint(x: x, y: f(y)), control1: CGPoint(x: a, y: f(b)), control2: CGPoint(x: c, y: f(d)))
        case .z: p.closeSubpath() } }
    ctx.addPath(p); ctx.fillPath()
    le(UInt16(W)); le(UInt16(H)); le(UInt16(ops.count))
    for o in ops { switch o {
        case let .m(x, y): out.append(0); lef(x); lef(y)
        case let .l(x, y): out.append(1); lef(x); lef(y)
        case let .c(a, b, c, d, x, y): out.append(3); [a, b, c, d, x, y].forEach(lef)
        case .z: out.append(4) } }
    let b = ctx.data!.bindMemory(to: UInt8.self, capacity: W * H)
    out.append(contentsOf: UnsafeBufferPointer(start: b, count: W * H))
}
try! out.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
print("\(shapes.count) shapes, \(out.count) bytes")
