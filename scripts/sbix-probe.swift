// How CoreText draws an sbix colour bitmap: which strike it takes for a size,
// and what Apple's undocumented graphic type `flip` does: a colour bitmap is
// drawn from a strike that holds it, and `flip` is a mirror. This file
// reproduces most of the numbers that rest on, though not all of them.
//
// NOTHING IS DRAWN ON SCREEN AND NOTHING IS POSTED. Every glyph is drawn with
// CTFontDrawGlyphs into an offscreen CGBitmapContext: sRGB, 8-bit RGBA,
// premultiplied, at scale 1, so a font size in points is a ppem in pixels.
//
// The font's own tables -- the TTC header, maxp, head, hhea, hmtx, cmap
// (formats 4 and 12) and sbix -- are parsed here, so the reference images are
// the bytes of the file CoreText draws from. The default is face 0 of
// /System/Library/Fonts/Apple Color Emoji.ttc, opened by name with
// CTFontCreateWithName("AppleColorEmoji"); the probe refuses to run if
// CoreText's font is not that file or has another glyph count.
// FONT_FILE=<path> loads a file through CGFont and
// CTFontCreateWithGraphicsFont instead -- the path scripts/sbix-probe-fonts.py
// builds fonts for. That path has none of the system font's own placement:
// no drop of an eighth of the size and no enlargement below 24 px.
//
// HOW A NUMBER IS TAKEN
//   A candidate is a strike's image -- decoded by ImageIO, after following
//   any `dupe` and `flip` records to the image they name -- drawn with
//   CoreGraphics at .high interpolation into the same context. The score is
//   the mean absolute error per channel (0-255) over every pixel either image
//   covers. A comparison where neither has a pixel is no comparison and is
//   not printed as a match. "CoreText used this strike" means an error of
//   exactly 0.000, with the runner-up printed beside it.
//
// MODES (glyph lists and size lists are comma-separated; omit for defaults;
// a glyph is a decimal glyph id or U+XXXX through the font's cmap)
//
//   strikes [glyphs] [sizes]
//     For each glyph and size, every strike's image is fitted: the drawn
//     square's side w from S-2 to ceil(1.3 S)+1 px, its lower-left corner at
//     dx -2..2 and dy -b-3..-b+3 from the glyph origin, b = round(S/8) on the
//     system font and 0 under FONT_FILE. Prints each cell's best strike and
//     runner-up with their errors and (w, dx, dy), whether the strike the rule
//     names for that w agrees -- the smallest strike at or above w that holds
//     the glyph, else the largest that does -- and a markdown table. Defaults:
//     U+1F525, U+1F600, U+1FAE0 (glyph 3578, empty at 40/48/52) and U+1FA77
//     (glyph 3471, empty at 20/26/40/48/52), at 20 24 26 30 32 36 40 44 48 50
//     52 56 60 64 px. About 5 s of one core for the defaults.
//
//   flip [glyphs|all] [sizes]
//     For each glyph, the chain of records it names in the strike the rule
//     picks for E = the size rounded half up. Then the error of the chain's
//     image drawn five ways: upright, mirrored within the image, mirrored
//     about the origin, flipped vertically and rotated 180 degrees. Each
//     draw's lower-left corner is searched over dx -12..12 and dy -b-3..-b+3
//     on the system font (b = round(E/8)), or -8..8 under FONT_FILE. Every
//     horizontal mirror is a translation of every other, so the axis is not
//     read off that search. It is read at fixed positions, each scored with
//     no search:
//       - the image mirrored in its own width at the chain end's offsets;
//       - the same at the first record's own offsets;
//       - mirrored about the first glyph's advance;
//       - mirrored about the chain end glyph's advance;
//       - mirrored about the origin;
//       - upright at the chain end's offsets.
//     Offsets are scaled by E/ppem and rounded half up. The advances are
//     CoreText's own (CTFontGetAdvancesForGlyphs), printed beside hmtx's, so
//     an edit that never reached the advance shows. Defaults: 838 (flip to
//     837), 839 (to 836) and 841 (to 840), at 64, 160, 56 and 40 px. `all`
//     takes every glyph that is a `flip` in any strike, 108 in face 0; run it
//     at one native size (3 s at 64). Sizes under 24 px are refused on the
//     system font, where CoreText enlarges the image by a rule only fitted.
//
//   fallback [glyphs] [sizes]
//     For each glyph and size, the error of every strike that holds the
//     glyph, drawn at w = E (dx, dy -2..2 around the system drop), the strike
//     CoreText matched, and the strike the rule predicts. A glyph no strike
//     holds must draw 0 pixels, and says so. Defaults: U+1F601..U+1F605
//     (glyphs 2097-2101) at 16 20 24 32 40 52 56 60 64 80 100 160 200 px.
//
// A run ends with a summary that says when the probe could not tell the
// cases apart: a runner-up that also reads 0.000, or an upright draw within
// 10 of a mirror.
//
// Build and run:
//   env -u DEVELOPER_DIR -u SDKROOT /usr/bin/swiftc -O scripts/sbix-probe.swift -o sbix-probe
//   ./sbix-probe strikes
//   ./sbix-probe flip
//   FONT_FILE=<dir>/sbix-a.ttf ./sbix-probe flip 838,852,846,839,840,841,842,845,844,847,867,868 64,40
// The devenv shell's DEVELOPER_DIR names a nix SDK with no Swift toolchain,
// hence the env -u. scripts/sbix-probe-fonts.py builds the modified fonts and
// prints the exact commands for each.

import CoreGraphics
import CoreText
import Foundation
import ImageIO

func die(_ m: String) -> Never {
    FileHandle.standardError.write((m + "\n").data(using: .utf8)!)
    exit(2)
}

let env = ProcessInfo.processInfo.environment
let systemPath = "/System/Library/Fonts/Apple Color Emoji.ttc"
let fontFile = env["FONT_FILE"]
let onSystem = fontFile == nil
let fontPath = fontFile ?? systemPath

// ---- the font's own tables ----------------------------------------------

struct Bytes {
    let d: Data
    func u8(_ o: Int) -> Int { Int(d[d.startIndex + o]) }
    func u16(_ o: Int) -> Int { u8(o) << 8 | u8(o + 1) }
    func i16(_ o: Int) -> Int { let v = u16(o); return v >= 0x8000 ? v - 0x10000 : v }
    func u32(_ o: Int) -> Int { u16(o) << 16 | u16(o + 2) }
    func tag(_ o: Int) -> String {
        String(bytes: d[(d.startIndex + o)..<(d.startIndex + o + 4)], encoding: .isoLatin1)!
    }
    func sub(_ o: Int, _ n: Int) -> Data { d.subdata(in: (d.startIndex + o)..<(d.startIndex + o + n)) }
}

/// One glyph record in one strike: its graphic type, its origin offsets in
/// strike pixels, and where its payload lies in the file.
struct Rec {
    let glyph: Int
    let type: String
    let ox: Int
    let oy: Int
    let off: Int
    let len: Int
}

struct Strike {
    let ppem: Int
    let ppi: Int
    let off: Int
}

final class Face {
    let b: Bytes
    var tables: [String: (off: Int, len: Int)] = [:]
    var numGlyphs = 0, upem = 0, nhm = 0, hmtx = 0
    var strikes: [Strike] = []
    var groups: [(lo: Int, hi: Int, g: Int)] = []  // cmap format 12
    var fmt4: Int? = nil

    init(_ path: String) {
        guard let d = try? Data(contentsOf: URL(fileURLWithPath: path), options: .alwaysMapped) else {
            die("cannot read \(path)")
        }
        b = Bytes(d: d)
        let face = b.tag(0) == "ttcf" ? b.u32(12) : 0  // face 0 of a collection
        for i in 0..<b.u16(face + 4) {
            let r = face + 12 + 16 * i
            tables[b.tag(r)] = (b.u32(r + 8), b.u32(r + 12))
        }
        for t in ["maxp", "head", "hhea", "hmtx", "sbix", "cmap"] where tables[t] == nil {
            die("\(path) has no \(t) table")
        }
        numGlyphs = b.u16(tables["maxp"]!.off + 4)
        upem = b.u16(tables["head"]!.off + 18)
        nhm = b.u16(tables["hhea"]!.off + 34)
        hmtx = tables["hmtx"]!.off
        let (sb, sblen) = tables["sbix"]!
        let n = b.u32(sb + 4)
        guard 8 + 4 * n <= sblen else { die("sbix claims \(n) strikes, more than its table holds") }
        for s in 0..<n {
            let so = sb + b.u32(sb + 8 + 4 * s)
            strikes.append(Strike(ppem: b.u16(so), ppi: b.u16(so + 2), off: so))
        }
        let cm = tables["cmap"]!.off
        for i in 0..<b.u16(cm + 2) {
            let st = cm + b.u32(cm + 4 + 8 * i + 4)
            switch b.u16(st) {
            case 12 where groups.isEmpty:
                for k in 0..<b.u32(st + 12) {
                    let r = st + 16 + 12 * k
                    groups.append((b.u32(r), b.u32(r + 4), b.u32(r + 8)))
                }
            case 4 where fmt4 == nil: fmt4 = st
            default: break
            }
        }
    }

    func advance(_ g: Int) -> Int { b.u16(hmtx + 4 * min(g, nhm - 1)) }

    func record(_ si: Int, _ g: Int) -> Rec? {
        guard g >= 0, g < numGlyphs else { return nil }
        let so = strikes[si].off
        let a = b.u32(so + 4 + 4 * g), e = b.u32(so + 4 + 4 * (g + 1))
        guard e >= a + 8 else { return nil }  // empty: no data in this strike
        let r = so + a
        return Rec(glyph: g, type: b.tag(r + 4), ox: b.i16(r), oy: b.i16(r + 2), off: r + 8, len: e - a - 8)
    }

    func glyph(for cp: Int) -> Int? {
        for (lo, hi, g) in groups where cp >= lo && cp <= hi { return g + cp - lo }
        guard let st = fmt4, cp <= 0xFFFF else { return nil }
        let seg = b.u16(st + 6) / 2
        for i in 0..<seg {
            let end = b.u16(st + 14 + 2 * i)
            guard cp <= end else { continue }
            let start = b.u16(st + 16 + 2 * seg + 2 * i)
            guard cp >= start else { return nil }
            let delta = b.u16(st + 16 + 4 * seg + 2 * i)
            let roAt = st + 16 + 6 * seg + 2 * i
            let ro = b.u16(roAt)
            if ro == 0 { return (cp + delta) & 0xFFFF }
            let g = b.u16(roAt + ro + 2 * (cp - start))
            return g == 0 ? nil : (g + delta) & 0xFFFF
        }
        return nil
    }

    func scalar(of g: Int) -> Int? {
        for (lo, hi, g0) in groups where g >= g0 && g <= g0 + hi - lo { return lo + g - g0 }
        return nil
    }

    /// Strike indices from the smallest ppem to the largest.
    lazy var order: [Int] = strikes.indices.sorted { strikes[$0].ppem < strikes[$1].ppem }
}

let face = Face(fontPath)

/// A glyph's records in one strike, from the glyph to the image they name.
struct Chain {
    let links: [Rec]
    let flips: Int
    let image: CGImage
    var end: Rec { links.last! }
    var described: String {
        links.map { r in
            let t = r.type.trimmingCharacters(in: .whitespaces)
            return "\(r.glyph) \(t)(\(r.ox),\(r.oy))"
        }.joined(separator: " -> ")
    }
}

func decode(_ r: Rec) -> CGImage? {
    guard let src = CGImageSourceCreateWithData(face.b.sub(r.off, r.len) as CFData, nil) else { return nil }
    return CGImageSourceCreateImageAtIndex(src, 0, nil)
}

func resolve(_ si: Int, _ g: Int) -> Chain? {
    var links: [Rec] = []
    var seen = Set<Int>()
    var cur = g
    var flips = 0
    while true {
        guard !seen.contains(cur), let r = face.record(si, cur) else { return nil }
        seen.insert(cur)
        links.append(r)
        if r.type == "dupe" || r.type == "flip" {
            guard r.len >= 2 else { return nil }
            if r.type == "flip" { flips += 1 }
            cur = face.b.u16(r.off)
            continue
        }
        guard let img = decode(r) else { return nil }
        return Chain(links: links, flips: flips, image: img)
    }
}

/// The strike the measured rule names for a drawn square of side e: the
/// smallest at or above e that holds the glyph, else the largest that does.
func ruleStrike(_ g: Int, _ e: Double) -> Int? {
    for si in face.order where Double(face.strikes[si].ppem) >= e && resolve(si, g) != nil { return si }
    return face.order.reversed().first { resolve($0, g) != nil }
}

func roundHalfUp(_ x: Double) -> Double { (x + 0.5).rounded(.down) }

// ---- drawing -------------------------------------------------------------

let space = CGColorSpace(name: CGColorSpace.sRGB)!

final class Canvas {
    let side: Int
    let mem: UnsafeMutablePointer<UInt8>
    let ctx: CGContext
    init(_ side: Int) {
        self.side = side
        mem = UnsafeMutablePointer<UInt8>.allocate(capacity: side * side * 4)
        mem.initialize(repeating: 0, count: side * side * 4)
        ctx = CGContext(
            data: mem, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side * 4,
            space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    }
    func clear() { ctx.clear(CGRect(x: 0, y: 0, width: side, height: side)) }
    /// An explicit copy. A buffer the context writes through is invisible to
    /// Swift's copy-on-write; an earlier probe read 0.000 everywhere because
    /// its two "copies" were one buffer.
    func snapshot() -> [UInt8] { Array(UnsafeBufferPointer(start: mem, count: side * side * 4)) }
}

var ctFonts: [Double: CTFont] = [:]
let cgFont: CGFont? = fontFile.map { f in
    guard let p = CGDataProvider(filename: f), let cg = CGFont(p) else { die("CoreGraphics cannot load \(f)") }
    return cg
}

func ctFont(_ size: Double) -> CTFont {
    if let f = ctFonts[size] { return f }
    let f =
        cgFont.map { CTFontCreateWithGraphicsFont($0, CGFloat(size), nil, nil) }
        ?? CTFontCreateWithName("AppleColorEmoji" as CFString, CGFloat(size), nil)
    ctFonts[size] = f
    return f
}

func renderCT(_ c: Canvas, _ g: Int, _ size: Double, at o: Double) -> [UInt8] {
    c.clear()
    var glyph = CGGlyph(g)
    var p = CGPoint(x: o, y: o)
    CTFontDrawGlyphs(ctFont(size), &glyph, &p, 1, c.ctx)
    return c.snapshot()
}

func ctAdvance(_ g: Int, _ size: Double) -> Double {
    var glyph = CGGlyph(g)
    var adv = CGSize.zero
    CTFontGetAdvancesForGlyphs(ctFont(size), .horizontal, &glyph, &adv, 1)
    return Double(adv.width)
}

enum Mirror: String, CaseIterable {
    case none = "upright"
    case image = "mirrored in the image"
    case origin = "mirrored about the origin"
    case vertical = "vertical"
    case rotate180 = "rotated 180"
}

/// Draw `img` as a w x h rectangle whose lower-left corner is (x, y), under
/// one of the five transforms.
func draw(_ c: Canvas, _ img: CGImage, x: Double, y: Double, w: Double, h: Double, _ m: Mirror) {
    c.clear()
    let k = c.ctx
    k.saveGState()
    k.interpolationQuality = .high
    switch m {
    case .none: k.translateBy(x: x, y: y)
    case .image:
        k.translateBy(x: x + w, y: y)
        k.scaleBy(x: -1, y: 1)
    case .origin:
        k.translateBy(x: x, y: y)
        k.scaleBy(x: -1, y: 1)
    case .vertical:
        k.translateBy(x: x, y: y + h)
        k.scaleBy(x: 1, y: -1)
    case .rotate180:
        k.translateBy(x: x + w, y: y + h)
        k.scaleBy(x: -1, y: -1)
    }
    k.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
    k.restoreGState()
}

/// Mean absolute error per channel over every pixel either image covers;
/// nil when neither covers one.
func mae(_ a: [UInt8], _ c: Canvas) -> Double? {
    a.withUnsafeBufferPointer { ap -> Double? in
        let bp = c.mem
        var sum = 0, n = 0, i = 0
        while i < ap.count {
            if ap[i + 3] != 0 || bp[i + 3] != 0 {
                sum += abs(Int(ap[i]) - Int(bp[i])) + abs(Int(ap[i + 1]) - Int(bp[i + 1]))
                sum += abs(Int(ap[i + 2]) - Int(bp[i + 2])) + abs(Int(ap[i + 3]) - Int(bp[i + 3]))
                n += 1
            }
            i += 4
        }
        return n == 0 ? nil : Double(sum) / Double(4 * n)
    }
}

func opaque(_ a: [UInt8]) -> Int {
    var n = 0
    var i = 3
    while i < a.count {
        if a[i] != 0 { n += 1 }
        i += 4
    }
    return n
}

struct Fit {
    var e = Double.infinity
    var w = 0
    var dx = 0
    var dy = 0
}

func f3(_ x: Double) -> String { x.isFinite ? String(format: "%.3f", x) : "n/a" }

func label(_ g: Int) -> String {
    face.scalar(of: g).map { String(format: "glyph %d (U+%04X)", g, $0) } ?? "glyph \(g)"
}

func parseGlyphs(_ s: String) -> [Int] {
    s.split(separator: ",").map { t -> Int in
        let t = t.trimmingCharacters(in: .whitespaces)
        if t.uppercased().hasPrefix("U+") {
            guard let cp = Int(t.dropFirst(2), radix: 16), let g = face.glyph(for: cp) else {
                die("\(t) is not in the font's cmap")
            }
            return g
        }
        guard let g = Int(t), g >= 0, g < face.numGlyphs else { die("\(t) is not a glyph id of this font") }
        return g
    }
}

func parseSizes(_ s: String) -> [Double] {
    s.split(separator: ",").map { t in
        guard let v = Double(t), v > 0 else { die("\(t) is not a size") }
        return v
    }
}

// ---- the font CoreText opens is the file parsed ----------------------------

let probe = ctFont(40)
let ctGlyphs = CTFontGetGlyphCount(probe)
if onSystem {
    let url = CTFontCopyAttribute(probe, kCTFontURLAttribute) as? URL
    guard url?.path == systemPath else {
        die("CoreText's AppleColorEmoji is \(url?.path ?? "unknown"), not \(systemPath); the parsed bytes would not be what it draws")
    }
}
guard ctGlyphs == face.numGlyphs else {
    die("CoreText's font has \(ctGlyphs) glyphs; the parsed file has \(face.numGlyphs)")
}
print("font: \(fontPath) (face 0), \(face.numGlyphs) glyphs, upem \(face.upem)")
print("opened by: \(onSystem ? "CTFontCreateWithName(\"AppleColorEmoji\")" : "CGFont + CTFontCreateWithGraphicsFont")"
    + ", PostScript name \(CTFontCopyPostScriptName(probe) as String)")
print("sbix strikes (ppem): \(face.order.map { face.strikes[$0].ppem })")

let args = Array(CommandLine.arguments.dropFirst())
guard let mode = args.first else { die("usage: sbix-probe strikes|flip|fallback [glyphs] [sizes]") }
func arg(_ i: Int) -> String? { args.count > i && args[i] != "-" ? args[i] : nil }

// ---- strikes ---------------------------------------------------------------

func modeStrikes() {
    let glyphs = parseGlyphs(arg(1) ?? "U+1F525,U+1F600,U+1FAE0,U+1FA77")
    let sizes = parseSizes(arg(2) ?? "20,24,26,30,32,36,40,44,48,50,52,56,60,64")
    print("mode strikes: w from S-2 to ceil(1.3S)+1, dx -2..2, dy -b-3..-b+3 with b = \(onSystem ? "round(S/8)" : "0")")
    for g in glyphs {
        let held = face.order.filter { resolve($0, g) != nil }.map { face.strikes[$0].ppem }
        print("\(label(g)): images in strikes \(held)")
    }
    var cells: [[String]] = Array(repeating: [], count: sizes.count)
    var square: [String] = Array(repeating: "", count: sizes.count)
    var worstBest = 0.0, leastRunner = Double.infinity, agree = 0, total = 0, blind = 0
    for g in glyphs {
        for (row, S) in sizes.enumerated() {
            let side = Int((S * 3).rounded(.up))
            let c = Canvas(side)
            let o = S.rounded(.down)
            let ct = renderCT(c, g, S, at: o)
            guard opaque(ct) > 0 else {
                print(String(format: "%@ size %5.1f: CoreText drew nothing (0 px)", label(g), S))
                cells[row].append("nothing")
                continue
            }
            let b = onSystem ? Int((S / 8).rounded()) : 0
            var fits: [(si: Int, fit: Fit)] = []
            for si in face.order {
                guard let ch = resolve(si, g) else { continue }
                let ppem = Double(face.strikes[si].ppem)
                let m: Mirror = ch.flips > 0 ? .image : .none
                var best = Fit()
                for w in max(1, Int(S) - 2)...(Int((S * 1.3).rounded(.up)) + 1) {
                    let iw = Double(w) * Double(ch.image.width) / ppem
                    let ih = Double(w) * Double(ch.image.height) / ppem
                    for dy in (-b - 3)...(-b + 3) {
                        for dx in -2...2 {
                            draw(c, ch.image, x: o + Double(dx), y: o + Double(dy), w: iw, h: ih, m)
                            if let e = mae(ct, c), e < best.e { best = Fit(e: e, w: w, dx: dx, dy: dy) }
                        }
                    }
                }
                fits.append((si, best))
            }
            fits.sort { $0.fit.e < $1.fit.e }
            guard let top = fits.first else {
                print(String(format: "%@ size %5.1f: CoreText drew %d px, but no strike holds an image", label(g), S, opaque(ct)))
                continue
            }
            let run = fits.count > 1 ? fits[1] : nil
            let ruled = ruleStrike(g, Double(top.fit.w)).map { face.strikes[$0].ppem }
            let bestPpem = face.strikes[top.si].ppem
            total += 1
            if ruled == bestPpem { agree += 1 }
            worstBest = max(worstBest, top.fit.e)
            if let r = run {
                leastRunner = min(leastRunner, r.fit.e)
                if r.fit.e < 0.0005 { blind += 1 }
            }
            let all = fits.map { "\(face.strikes[$0.si].ppem):\(String(format: "%.2f", $0.fit.e))" }.joined(separator: " ")
            print(
                String(
                    format: "%@ size %5.1f px %5d  BEST %3d %@ (w %d dx %d dy %d)  runner-up %@  rule for w %d: %@  | %@",
                    label(g), S, opaque(ct), bestPpem, f3(top.fit.e), top.fit.w, top.fit.dx, top.fit.dy,
                    run.map { "\(face.strikes[$0.si].ppem) \(f3($0.fit.e))" } ?? "none", top.fit.w,
                    ruled.map { "\($0)" + ($0 == bestPpem ? " (agrees)" : " (DIFFERS)") } ?? "none", all))
            cells[row].append(
                "**\(bestPpem)** (\(f3(top.fit.e)))" + (run.map { " / \(face.strikes[$0.si].ppem) (\(f3($0.fit.e)))" } ?? ""))
            if square[row].isEmpty { square[row] = "\(top.fit.w) px, dy \(top.fit.dy)" }
        }
    }
    print("\n| px size | drawn square, y offset (first glyph) | " + glyphs.map { label($0) }.joined(separator: " | ") + " |")
    print("|---|---|" + glyphs.map { _ in "---|" }.joined())
    for (row, S) in sizes.enumerated() {
        print("| \(String(format: "%g", S)) | \(square[row]) | " + cells[row].joined(separator: " | ") + " |")
    }
    print(
        "\nsummary: \(total) cells; worst best match \(f3(worstBest)); least runner-up \(f3(leastRunner)); "
            + "the rule names the matched strike in \(agree) of \(total)")
    if blind > 0 { print("WARNING: \(blind) cells have a runner-up at 0.000; the probe cannot tell those strikes apart") }
}

// ---- flip ------------------------------------------------------------------

func modeFlip() {
    var glyphs: [Int]
    if arg(1) == "all" {
        glyphs = (0..<face.numGlyphs).filter { g in face.strikes.indices.contains { face.record($0, g)?.type == "flip" } }
        print("all: \(glyphs.count) glyphs are `flip` in some strike")
    } else {
        glyphs = parseGlyphs(arg(1) ?? "838,839,841")
    }
    let sizes = parseSizes(arg(2) ?? "64,160,56,40")
    let dyRange = onSystem ? "-b-3..-b+3 with b = round(E/8)" : "-8..8"
    print("mode flip: search dx -12..12, dy \(dyRange); the fixed positions are scored with no search")
    var worstMirror = 0.0, leastOther = Double.infinity, warnings = 0, cellsDone = 0
    for g in glyphs {
        for S in sizes {
            let E = roundHalfUp(S)
            if onSystem && E < 24 {
                print(String(format: "%@ size %5.1f: refused; below 24 px the system font enlarges the image", label(g), S))
                continue
            }
            guard let si = ruleStrike(g, E), let ch = resolve(si, g) else {
                let c = Canvas(Int((S * 3).rounded(.up)))
                let px = opaque(renderCT(c, g, S, at: S.rounded(.down)))
                print(String(format: "%@ size %5.1f: no strike holds an image; CoreText drew %d px", label(g), S, px))
                continue
            }
            let ppem = Double(face.strikes[si].ppem)
            let scale = E / ppem
            let iw = Double(ch.image.width) * scale
            let ih = Double(ch.image.height) * scale
            let c = Canvas(Int((S * 3).rounded(.up)))
            let o = S.rounded(.down)
            let ct = renderCT(c, g, S, at: o)
            let px = opaque(ct)
            print(
                String(
                    format: "%@ size %5.1f strike %d image %dx%d (drawn %gx%g) CoreText %d px; chain %@; %d flip%@",
                    label(g), S, Int(ppem), ch.image.width, ch.image.height, iw, ih, px, ch.described, ch.flips,
                    ch.flips == 1 ? "" : "s"))
            guard px > 0 else {
                print("    CoreText drew nothing")
                continue
            }
            cellsDone += 1
            let b = onSystem ? Int((E / 8).rounded()) : 0
            let dys = onSystem ? (-b - 3)...(-b + 3) : -8...8
            var line: [String] = []
            var errs: [Mirror: Double] = [:]
            for m in Mirror.allCases {
                var best = Fit()
                for dy in dys {
                    for dx in -12...12 {
                        draw(c, ch.image, x: o + Double(dx), y: o + Double(dy), w: iw, h: ih, m)
                        if let e = mae(ct, c), e < best.e { best = Fit(e: e, w: 0, dx: dx, dy: dy) }
                    }
                }
                errs[m] = best.e
                line.append("\(m.rawValue) \(f3(best.e)) (dx \(best.dx) dy \(best.dy))")
            }
            print("    searched: " + line.joined(separator: " | "))
            // The axis and the offsets, at fixed positions.
            let drop = -Double(b)
            let first = ch.links[0], end = ch.end
            let ex = roundHalfUp(Double(end.ox) * scale), ey = roundHalfUp(Double(end.oy) * scale)
            let fx = roundHalfUp(Double(first.ox) * scale), fy = roundHalfUp(Double(first.oy) * scale)
            // The advances in force are CoreText's; hmtx's are printed beside them,
            // since an edit that misses hmtx's long metrics changes nothing.
            let advFirst = ctAdvance(first.glyph, S), advEnd = ctAdvance(end.glyph, S)
            for (gl, a) in [(first.glyph, advFirst), (end.glyph, advEnd)] {
                let h = Double(face.advance(gl)) * S / Double(face.upem)
                print(String(format: "      advance of glyph %d: CoreText %g px, hmtx %d units = %g px%@", gl, a, face.advance(gl), h, abs(a - h) > 0.01 ? " (DIFFER)" : ""))
            }
            let y = o + ey + drop
            let fixed: [(String, Double, Double, Mirror)] = [
                ("upright at the chain end's offsets", o + ex, y, .none),
                ("mirrored in the image at the chain end's offsets", o + ex, y, .image),
                ("mirrored in the image at the first record's own offsets", o + fx, o + fy + drop, .image),
                ("mirrored about glyph \(first.glyph)'s advance (\(String(format: "%g", advFirst)) px)", o + advFirst - ex - iw, y, .image),
                ("mirrored about glyph \(end.glyph)'s advance (\(String(format: "%g", advEnd)) px)", o + advEnd - ex - iw, y, .image),
                ("mirrored about the origin", o - ex - iw, y, .image),
            ]
            var zero: [String] = []
            for (name, x, yy, m) in fixed {
                draw(c, ch.image, x: x, y: yy, w: iw, h: ih, m)
                let e = mae(ct, c) ?? .infinity
                print(String(format: "      %@: x %g, y %g -> %@", name, x - o, yy - o, f3(e)))
                if e < 0.0005 { zero.append(name) }
            }
            print("    at 0.000: " + (zero.isEmpty ? "none" : zero.joined(separator: "; ")))
            if ch.flips >= 2 {
                print("    \(ch.flips) flips: a mirror per flip predicts \(ch.flips % 2 == 0 ? "upright" : "mirrored"); one mirror for any flip predicts mirrored")
            }
            let up = errs[.none] ?? .infinity, mi = errs[.image] ?? .infinity
            if ch.flips > 0 {
                worstMirror = max(worstMirror, mi)
                for m in [Mirror.none, .origin, .vertical, .rotate180] { leastOther = min(leastOther, errs[m] ?? .infinity) }
            }
            if abs(up - mi) < 10 {
                warnings += 1
                print("    WARNING: upright and mirrored are within 10; this cell cannot tell them apart")
            }
        }
    }
    print(
        "\nsummary: \(cellsDone) cells drawn; with a flip in the chain, the worst mirrored-in-the-image error "
            + "\(f3(worstMirror)) and the least of the other four \(f3(leastOther))")
    if warnings > 0 { print("WARNING: \(warnings) cells could not tell upright from mirrored") }
}

// ---- fallback --------------------------------------------------------------

func modeFallback() {
    let glyphs = parseGlyphs(arg(1) ?? "U+1F601,U+1F602,U+1F603,U+1F604,U+1F605")
    let sizes = parseSizes(arg(2) ?? "16,20,24,32,40,52,56,60,64,80,100,160,200")
    print("mode fallback: each strike drawn at w = E, dx and dy -2..2 around the drop")
    var agree = 0, total = 0
    for g in glyphs {
        let held = face.order.filter { resolve($0, g) != nil }.map { face.strikes[$0].ppem }
        print("\(label(g)): images in strikes \(held)")
        for S in sizes {
            let E = roundHalfUp(S)
            if onSystem && E < 24 {
                print(String(format: "  size %5.1f: refused; below 24 px the system font enlarges the image", S))
                continue
            }
            let c = Canvas(Int((S * 3).rounded(.up)))
            let o = S.rounded(.down)
            let ct = renderCT(c, g, S, at: o)
            let px = opaque(ct)
            let ruled = ruleStrike(g, E).map { face.strikes[$0].ppem }
            total += 1
            guard px > 0 else {
                let ok = ruled == nil
                if ok { agree += 1 }
                print(
                    String(format: "  size %5.1f: CoreText drew 0 px; the rule predicts %@", S, ruled.map { "strike \($0) (DIFFERS)" } ?? "nothing (agrees)"))
                continue
            }
            let b = onSystem ? Int((E / 8).rounded()) : 0
            var res: [(ppem: Int, e: Double)] = []
            for si in face.order {
                guard let ch = resolve(si, g) else { continue }
                let ppem = Double(face.strikes[si].ppem)
                let iw = Double(ch.image.width) * E / ppem, ih = Double(ch.image.height) * E / ppem
                var best = Double.infinity
                for dy in (-b - 2)...(-b + 2) {
                    for dx in -2...2 {
                        draw(c, ch.image, x: o + Double(dx), y: o + Double(dy), w: iw, h: ih, ch.flips > 0 ? .image : .none)
                        if let e = mae(ct, c) { best = min(best, e) }
                    }
                }
                res.append((Int(ppem), best))
            }
            res.sort { $0.e < $1.e }
            guard let top = res.first else {
                print(String(format: "  size %5.1f: CoreText drew %d px, but no strike holds an image", S, px))
                continue
            }
            if ruled == top.ppem && top.e < 0.0005 { agree += 1 }
            print(
                String(
                    format: "  size %5.1f px %5d -> strike %3d %@; the rule predicts %@ | %@", S, px, top.ppem, f3(top.e),
                    ruled.map { "\($0)" + ($0 == top.ppem ? " (agrees)" : " (DIFFERS)") } ?? "nothing (DIFFERS)",
                    res.map { "\($0.ppem):\(String(format: "%.2f", $0.e))" }.joined(separator: " ")))
        }
    }
    print("\nsummary: the rule and CoreText agree, at 0.000 where something was drawn, in \(agree) of \(total)")
}

switch mode {
case "strikes": modeStrikes()
case "flip": modeFlip()
case "fallback": modeFallback()
default: die("usage: sbix-probe strikes|flip|fallback [glyphs] [sizes]")
}
