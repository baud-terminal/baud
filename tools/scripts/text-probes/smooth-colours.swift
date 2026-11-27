// smooth-colours: CoreText's smoothing of a full block in coloured fills, for baud's level rule.
//
//   smooth-colours <font.ttf> <out.bin>
//
// The level of CoreText's dilation is a function of the fill colour's luminance; the grey fixtures
// (`smooth-fixtures.swift`) cannot say which luminance, because a grey has every channel equal. These
// fills tell Rec.709 from Rec.601 weights: pure green is level 3 under Rec.709 and 2 under Rec.601,
// magenta 1 and 2, ANSI green (0,205,0) 2 and 1. Each is drawn over black in an sRGB RGB context with
// smoothing off and on, and the channel at full intensity is read as coverage -- over black,
// OVER on stored values leaves `a * c` in each channel (`blend.swift`).
//
// File, little-endian: "BCOL" | u16 version=1 | u16 entry count | u32 font byte length | u32 font FNV-1a
//   entry: f32 ppem | u8 r, g, b | u16 w | u16 h | i16 pen_x | i16 pen_y | mask off (w*h u8) | mask on
// The masks are U+2588, rows top-down; pen as in the grey fixtures.
import CoreGraphics
import CoreText
import Foundation

let args = CommandLine.arguments
let fontData = try! Data(contentsOf: URL(fileURLWithPath: args[1]))
let cgFont = CGFont(CGDataProvider(data: fontData as CFData)!)!
func fnv(_ d: Data) -> UInt32 { var h: UInt32 = 0x811c9dc5; for b in d { h = (h ^ UInt32(b)) &* 0x01000193 }; return h }

// Primaries, secondaries, xterm's ANSI colours, and a few mid-tones.
let colours: [(UInt8, UInt8, UInt8)] = [
    (255, 0, 0), (0, 255, 0), (0, 0, 255), (0, 255, 255), (255, 0, 255), (255, 255, 0),
    (205, 0, 0), (0, 205, 0), (205, 205, 0), (0, 0, 238), (205, 0, 205), (0, 205, 205),
    (255, 85, 85), (85, 255, 85), (92, 92, 255), (255, 85, 255), (85, 255, 255), (255, 255, 85),
    (255, 128, 0), (128, 0, 255), (0, 128, 128),
]
let sizes: [Double] = [13.001, 20, 28]

func render(_ f: CTFont, _ g: CGGlyph, _ c: (UInt8, UInt8, UInt8), smooth: Bool, W: Int, H: Int, pen: (Int, Int)) -> [UInt8] {
    let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 4 * W,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.setFillColor(CGColor(srgbRed: 0, green: 0, blue: 0, alpha: 1)); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
    ctx.setShouldAntialias(true); ctx.setAllowsFontSmoothing(true); ctx.setShouldSmoothFonts(smooth)
    ctx.setAllowsFontSubpixelPositioning(false); ctx.setShouldSubpixelPositionFonts(false)
    ctx.setAllowsFontSubpixelQuantization(false); ctx.setShouldSubpixelQuantizeFonts(false)
    ctx.setFillColor(CGColor(srgbRed: CGFloat(c.0) / 255, green: CGFloat(c.1) / 255, blue: CGFloat(c.2) / 255, alpha: 1))
    ctx.textMatrix = .identity
    var gg = [g]; var p = [CGPoint(x: pen.0, y: H - pen.1)]
    CTFontDrawGlyphs(f, &gg, &p, 1, ctx)
    // The brightest channel carries the coverage at the finest step.
    let ch = c.0 >= c.1 && c.0 >= c.2 ? 0 : (c.1 >= c.2 ? 1 : 2)
    let full = Double([c.0, c.1, c.2][ch])
    let b = ctx.data!.bindMemory(to: UInt8.self, capacity: 4 * W * H)
    return (0..<(W * H)).map { UInt8(min(255, (Double(b[4 * $0 + ch]) * 255 / full).rounded())) }
}

var body = Data(); var count = 0
func le<T: FixedWidthInteger>(_ v: T, _ d: inout Data) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
for px in sizes {
    let f = CTFontCreateWithGraphicsFont(cgFont, CGFloat(px), nil, nil)
    var u = [UniChar(0x2588)]; var g = [CGGlyph(0)]; precondition(CTFontGetGlyphsForCharacters(f, &u, &g, 1))
    let W = Int(1.4 * px) + 16, H = Int(1.8 * px) + 16; let pen = (8, Int((1.3 * px).rounded(.up)) + 8)
    for c in colours {
        let off = render(f, g[0], c, smooth: false, W: W, H: H, pen: pen)
        let on = render(f, g[0], c, smooth: true, W: W, H: H, pen: pen)
        le(Float(px).bitPattern, &body); body.append(contentsOf: [c.0, c.1, c.2])
        le(UInt16(W), &body); le(UInt16(H), &body); le(Int16(pen.0), &body); le(Int16(pen.1), &body)
        body.append(contentsOf: off); body.append(contentsOf: on); count += 1
    }
}
var file = Data("BCOL".utf8)
le(UInt16(1), &file); le(UInt16(count), &file); le(UInt32(fontData.count), &file); le(fnv(fontData), &file)
file.append(body)
try! file.write(to: URL(fileURLWithPath: args[2]))
print("\(count) entries, \(file.count) bytes")
