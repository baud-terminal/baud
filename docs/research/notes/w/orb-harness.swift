// Research harness for the ORB trail. Scratch only; nothing in baud is touched.
// Reads baud's shaders.metal (read-only) for glyph_vertex, EffectU, fx_* and the
// fire, appends orb.metal, and holds the GPU to a CPU reference of the field.
// Build: DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -O main.swift -o orbh
import CoreGraphics
import Foundation
import ImageIO
import Metal
import UniformTypeIdentifiers

let here = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "."
let safe = CommandLine.arguments.contains("safe")
let dev = MTLCreateSystemDefaultDevice()!
let queue = dev.makeCommandQueue()!

// ---------------------------------------------------------------- the shader
let baud = try! String(contentsOfFile: "baud-present-macos/src/shaders.metal", encoding: .utf8)
func slice(_ s: String, from a: String, to b: String) -> String {
    let r0 = s.range(of: a)!, r1 = s.range(of: b, range: r0.upperBound ..< s.endIndex)!
    return String(s[r0.lowerBound ..< r1.lowerBound])
}
let head = slice(baud, from: "#include <metal_stdlib>", to: "fragment float4 glyph_fragment")
let fx = slice(baud, from: "struct EffectU", to: "// ---- The cell path")
let orbSrc = try! String(contentsOfFile: here + "/orb.metal", encoding: .utf8)
let source = head + "\n" + fx + "\n" + orbSrc
let lib: MTLLibrary = {
    let o = MTLCompileOptions()
    if safe { o.mathMode = .safe; o.mathFloatingPointFunctions = .precise }
    do { return try dev.makeLibrary(source: source, options: o) } catch { fatalError("\(error)") }
}()
func pipeline(_ frag: String) -> MTLRenderPipelineState {
    let d = MTLRenderPipelineDescriptor()
    d.vertexFunction = lib.makeFunction(name: "glyph_vertex")
    d.fragmentFunction = lib.makeFunction(name: frag)
    let a = d.colorAttachments[0]!
    a.pixelFormat = .bgra8Unorm
    a.isBlendingEnabled = true
    a.rgbBlendOperation = .add; a.alphaBlendOperation = .add
    a.sourceRGBBlendFactor = .one; a.sourceAlphaBlendFactor = .one
    a.destinationRGBBlendFactor = .oneMinusSourceAlpha; a.destinationAlphaBlendFactor = .oneMinusSourceAlpha
    return try! dev.makeRenderPipelineState(descriptor: d)
}
let orbPipe = pipeline("orb_fragment")
let firePipe = pipeline("fire_fragment")

// ---------------------------------------------------------------- geometry
struct Pt { var x: Float; var y: Float }
func + (a: Pt, b: Pt) -> Pt { Pt(x: a.x + b.x, y: a.y + b.y) }
func - (a: Pt, b: Pt) -> Pt { Pt(x: a.x - b.x, y: a.y - b.y) }
func * (a: Pt, k: Float) -> Pt { Pt(x: a.x * k, y: a.y * k) }
func len(_ a: Pt) -> Float { (a.x * a.x + a.y * a.y).squareRoot() }
struct RectF { var l: Float; var t: Float; var r: Float; var b: Float
    var corners: [Pt] { [Pt(x: r, y: t), Pt(x: r, y: b), Pt(x: l, y: b), Pt(x: l, y: t)] }
    var center: Pt { Pt(x: (l + r) / 2, y: (t + b) / 2) }
    func contains(_ p: Pt) -> Bool { p.x >= l && p.x < r && p.y >= t && p.y < b }
}
let CW: Float = 17, CH: Float = 34
func cell(_ c: Float, _ r: Float) -> RectF { RectF(l: c * CW, t: r * CH, r: (c + 1) * CW, b: (r + 1) * CH) }
func bar(_ c: Float, _ r: Float) -> RectF { RectF(l: c * CW, t: r * CH, r: c * CW + 2, b: (r + 1) * CH) }

let TAU_FAST: Float = 0.1, TAU_SLOW: Float = 0.4
func exp2f_(_ x: Float) -> Float { Foundation.exp2(x) }
struct Segment {
    var from: [Pt]; var to: RectF; var tau: [Float]
    init(_ from: [Pt], _ to: RectF) {
        self.from = from; self.to = to
        // The rates as note W, section 8.4, derives them: a corner leads by
        // the cosine of its travel and its outward direction; one that does
        // not travel takes no part.
        let target = to.corners, c = to.center
        let leads: [Float?] = (0 ..< 4).map { i in
            let d = target[i] - from[i], o = target[i] - c
            let travel = len(d), out = len(o)
            return travel >= 1e-3 && out > 0 ? (d.x * o.x + d.y * o.y) / (travel * out) : nil
        }
        let live = leads.compactMap { $0 }
        let lo = live.min() ?? 0, hi = live.max() ?? 0
        tau = leads.map { l in
            guard let l = l, hi - lo > Float.ulpOfOne else { return TAU_SLOW }
            let s = (l - lo) / (hi - lo)
            return TAU_FAST * s + TAU_SLOW * (1 - s)
        }
    }
    func at(_ t: Float) -> [Pt] {
        let target = to.corners
        return (0 ..< 4).map { i in
            let k = exp2f_(-10 * max(t, 0) / tau[i])
            return target[i] + (from[i] - target[i]) * k
        }
    }
}
// The lead: a fifth point of kitty's chase, on the fast constant, chasing the
// cursor's centre from wherever it was when this segment started.
struct Lead {
    var from: Pt; var to: Pt
    func at(_ t: Float) -> Pt { to + (from - to) * exp2f_(-10 * max(t, 0) / TAU_FAST) }
    var lasts: Float { let g = max(abs(to.x - from.x), abs(to.y - from.y)); return g > 0.5 ? TAU_FAST / 10 * log2(g / 0.5) : 0 }
}

// ---------------------------------------------------------------- the orb's numbers (Rust-side)
let RADIUS: Float = 0.35      // cells of height
let TAU_GLOW: Float = 0.1     // glow rise, kitty's form
let ABSORB: Float = 0.12      // s, orb fades into the cursor after landing
let PULSE: Float = 0.20       // s, the landing ring
let RING: Float = 1.5         // cells, the ring's final radius (Neovide's sonicboom: 3 heights across)
let K_RING: Float = 1.2
let REACH: Float = 2.0
let MOTES = 5
let TILT: Float = 0.45

struct Hash { var v: UInt32
    func next() -> Hash {
        let s = v &* 747_796_405 &+ 2_891_336_453
        let w = ((s >> ((s >> 28) &+ 4)) ^ s) &* 277_803_737
        return Hash(v: (w >> 22) ^ w)
    }
    var unit: Float { Float(bitPattern: (v >> 9) | 0x3f80_0000) - 1 }
}

// One frame of the driver: the quad's corners, the lead, glow, ring.
struct Frame {
    var corners: [Pt]; var hole: RectF; var lead: Pt; var glow: Float
    var ring: Pt; var ringR: Float; var ringA: Float
    var tail: Pt; var dir: Pt; var len: Float; var age: Float; var seed: UInt32
}
// A scripted timeline: ignite at 0 from `from` to `to`, optionally retarget at tr to `to2`.
struct Script { var from: RectF; var to: RectF; var retarget: (Float, RectF)? }
func frame(_ s: Script, _ t: Float, seed: UInt32 = 7) -> Frame {
    let c = CH
    var seg = Segment(s.from.corners, s.to)
    var lead = Lead(from: s.from.center, to: s.to.center)
    var start: Float = 0, g0: Float = 0, hole = s.to
    if let (tr, to2) = s.retarget, t >= tr {
        let corners = seg.at(tr)
        let lp = lead.at(tr)
        let g = glowAt(lead: lead, start: 0, g0: 0, t: tr)
        seg = Segment(corners, to2); lead = Lead(from: lp, to: to2.center)
        start = tr; g0 = g; hole = to2
    }
    let glow = glowAt(lead: lead, start: start, g0: g0, t: t)
    let tl = t - (start + lead.lasts)
    var ringR: Float = 0, ringA: Float = 0
    if tl >= 0 && tl < PULSE {
        let q = tl / PULSE, R = RADIUS * c
        ringR = R + (RING * c - R) * (1 - (1 - q) * (1 - q)).squareRoot()
        ringA = K_RING * (1 - q) * (1 - q)
    }
    let d = s.to.center - s.from.center, L = max(len(d), 1)
    return Frame(corners: seg.at(t - start), hole: hole, lead: lead.at(t - start), glow: glow,
                 ring: lead.to, ringR: ringR, ringA: ringA,
                 tail: s.from.center, dir: d * (1 / L), len: L, age: t, seed: seed)
}
func glowAt(lead: Lead, start: Float, g0: Float, t: Float) -> Float {
    let tp = t - start, land = lead.lasts
    let rise = { (x: Float) in 1 - (1 - g0) * exp2f_(-10 * max(x, 0) / TAU_GLOW) }
    if tp < land { return rise(tp) }
    let g1 = rise(land), q = min(max((tp - land) / ABSORB, 0), 1)
    return g1 * (1 - q) * (1 - q)
}

// Motes: five, orbiting the lead in a tilted ring about the move's axis.
struct Mote { var centre: Pt; var radius: Float; var glow: Float }
func motes(_ f: Frame) -> [Mote] {
    let c = CH, R = RADIUS * c
    let perp = Pt(x: -f.dir.y, y: f.dir.x)
    return (0 ..< MOTES).map { i in
        let h0 = Hash(v: f.seed ^ (UInt32(i) &* 0x9E37_79B9)).next()
        let h1 = h0.next(), h2 = h1.next(), h3 = h2.next()
        let th = 2 * Float.pi * h0.unit + (16 + 8 * h1.unit) * f.age
        let orbit = R * (1.4 + 0.5 * h2.unit) * (0.25 + 0.75 * f.glow)
        let off = perp * (cos(th) * orbit) + f.dir * (TILT * sin(th) * orbit)
        let z = sin(th)
        return Mote(centre: f.lead + off, radius: c * (0.05 + 0.025 * h3.unit) * (0.6 + 0.4 * f.glow),
                    glow: 0.85 * f.glow * (0.6 + 0.4 * z))
    }
}

// ---------------------------------------------------------------- records (effect.rs, ported)
struct Rec { var x: Int16; var y: Int16; var w: UInt16; var h: UInt16; var u: UInt16 = 0; var v: UInt16 = 0; var color: UInt32; var kind: UInt32 }
func chan(_ x: Float) -> UInt32 { UInt32(max(0, min(255, (x * 255).rounded()))) }
func records(_ f: Frame, fill: UInt32, frame: (Float, Float), reach: Float = REACH, withMotes: Bool = true) -> [Rec] {
    var out = [Rec]()
    let unit = CH, r = reach * unit, (fw, fh) = frame, q = f.corners
    let y0 = max(q.map { $0.y }.min()! - r, 0), y1 = min(q.map { $0.y }.max()! + r, fh)
    var band = (y0 / unit).rounded(.down) * unit
    while band < y1 {
        let lo = band - r, hi = band + unit + r
        var xmin = Float.infinity, xmax = -Float.infinity
        for i in 0 ..< 4 {
            let a = q[i], b = q[(i + 1) % 4]
            var t0: Float = 0, t1: Float = 1
            let dy = b.y - a.y
            if abs(dy) < 1e-6 { if a.y < lo || a.y > hi { continue } } else {
                let ta = (lo - a.y) / dy, tb = (hi - a.y) / dy
                t0 = max(t0, min(ta, tb)); t1 = min(t1, max(ta, tb)); if t0 > t1 { continue }
            }
            for t in [t0, t1] { let x = a.x + (b.x - a.x) * t; xmin = min(xmin, x); xmax = max(xmax, x) }
        }
        let x0 = max(xmin - r, 0).rounded(.down), x1 = min(xmax + r, fw).rounded(.up)
        let top = max(band, 0).rounded(.down), bottom = min(band + unit, fh).rounded(.up)
        if xmin <= xmax && x1 > x0 && bottom > top {
            out.append(Rec(x: Int16(x0), y: Int16(top), w: UInt16(x1 - x0), h: UInt16(bottom - top), color: fill | 0xFF00_0000, kind: 4))
        }
        band += unit
    }
    if withMotes {
        for m in motes(f) {
            let side = max((2 * m.radius).rounded(.up), 2)
            out.append(Rec(x: Int16((m.centre.x - side * 0.5).rounded(.down)), y: Int16((m.centre.y - side * 0.5).rounded(.down)),
                           w: UInt16(side), h: UInt16(side), color: (fill & 0x00FF_FFFF) | (chan(m.glow) << 24), kind: 5))
        }
    }
    return out
}
func recBytes(_ rs: [Rec]) -> [UInt8] {
    var out = [UInt8]()
    for r in rs {
        for v in [UInt16(bitPattern: r.x), UInt16(bitPattern: r.y), r.w, r.h, r.u, r.v] { out += [UInt8(v & 0xff), UInt8(v >> 8)] }
        for v in [r.color, r.kind] { out += [UInt8(v & 0xff), UInt8((v >> 8) & 0xff), UInt8((v >> 16) & 0xff), UInt8(v >> 24)] }
    }
    return out
}
// EffectU: 24-float prefix, then k[8].
func uniform(_ f: Frame, fire: Bool = false) -> [Float] {
    let c = f.corners, h = f.hole
    let k: [Float] = fire ? [1.6, 2.2, 0.6, 0, 0, 0, 0, 0]
        : [f.lead.x, f.lead.y, RADIUS * CH, f.glow, f.ring.x, f.ring.y, f.ringR, f.ringA]
    return [c[0].x, c[0].y, c[1].x, c[1].y, c[2].x, c[2].y, c[3].x, c[3].y, h.l, h.t, h.r, h.b, 0, 0,
            f.tail.x, f.tail.y, f.dir.x, f.dir.y, f.len, CH, f.age, fire ? 0.45 : 1.0, Float(bitPattern: f.seed), REACH] + k
}

// ---------------------------------------------------------------- the CPU reference
func sq(_ x: Float) -> Float { x * x }
func sat(_ x: Float) -> Float { min(max(x, 0), 1) }
func smoothstep(_ a: Float, _ b: Float, _ x: Float) -> Float { let t = sat((x - a) / (b - a)); return t * t * (3 - 2 * t) }
func quadDistance(_ p: Pt, _ q: [Pt]) -> Float {
    var d = Float.infinity; var w = 0
    for i in 0 ..< 4 {
        let a = q[i], b = q[(i + 1) % 4]
        let bx = b.x - a.x, by = b.y - a.y, px = p.x - a.x, py = p.y - a.y
        let h = sat((px * bx + py * by) / max(bx * bx + by * by, 1e-6))
        let dx = px - bx * h, dy = py - by * h
        d = min(d, (dx * dx + dy * dy).squareRoot())
        let side = bx * py - by * px
        if a.y <= p.y { if b.y > p.y && side > 0 { w += 1 } } else if b.y <= p.y && side < 0 { w -= 1 }
    }
    return w != 0 ? -d : d
}
// The literals orb.metal carries.
let K_CORE: Float = 2.0, K_BODY: Float = 0.9, K_RIM: Float = 1.3, HALO_A: Float = 0.8, K_HALO: Float = 0.9
let RIBBON: Float = 0.5, GLOW_OUT: Float = 0.25, WAKE_LEN: Float = 2.5, K_WAKE: Float = 1.0, RING_W: Float = 0.07
func density(_ p: Pt, _ f: Frame, cut: Bool = true) -> Float {
    let c = CH
    let d = quadDistance(p, f.corners)
    if cut && d > REACH * c { return 0 }
    let R = RADIUS * c
    let r = len(p - f.lead)
    let mu = max(1 - sq(r / R), 0).squareRoot(), m = 1 - mu
    let edge = sat(R - r + 0.5)
    let body = K_CORE * mu * mu * mu + K_BODY * mu + K_RIM * (m * m * m * m * m) * edge
    let halo = K_HALO / sq(1 + sq(r / (HALO_A * R)))
    let depth = sat(-d / (RIBBON * c))
    let wake = d <= 0 ? 0.25 + 0.75 * depth : 0.25 * exp(-sq(d / (GLOW_OUT * c)))
    let along = exp(-r / (WAKE_LEN * c))
    var D = f.glow * (body + halo + K_WAKE * wake * along)
    if f.ringA > 0 { D += f.ringA * exp(-sq((len(p - f.ring) - f.ringR) / (RING_W * c))) }
    return D
}
func unitColor(_ c: UInt32) -> [Float] { [Float((c >> 16) & 0xff) / 255, Float((c >> 8) & 0xff) / 255, Float(c & 0xff) / 255, Float(c >> 24) / 255] }
func over(_ s: [Float], _ dst: UInt32) -> UInt32 {
    let ch = { (k: UInt32) in Float((dst >> k) & 0xff) / 255 }
    let m = { (sv: Float, dv: Float) -> UInt32 in UInt32(max(0, min(255, ((sv + dv * (1 - s[3])) * 255).rounded()))) }
    return (m(s[3], ch(24)) << 24) | (m(s[0], ch(16)) << 16) | (m(s[1], ch(8)) << 8) | m(s[2], ch(0))
}
func reference(_ f: Frame, _ recs: [Rec], plain: [UInt32], W: Int) -> [UInt32] {
    var out = plain
    for r in recs {
        let col = unitColor(r.color)
        for y in max(Int(r.y), 0) ..< Int(r.y) + Int(r.h) {
            for x in max(Int(r.x), 0) ..< Int(r.x) + Int(r.w) {
                if x >= W || y >= plain.count / W { continue }
                let p = Pt(x: Float(x) + 0.5, y: Float(y) + 0.5)
                var src: [Float] = [0, 0, 0, 0]
                if !f.hole.contains(p) {
                    var a: Float
                    if r.kind == 5 {
                        let ux = (p.x - Float(r.x)) / Float(r.w) * 2 - 1, uy = (p.y - Float(r.y)) / Float(r.h) * 2 - 1
                        a = 1 - smoothstep(0.15, 1, (ux * ux + uy * uy).squareRoot()); a = col[3] * a * a
                    } else {
                        a = col[3] * (1 - exp(-density(p, f)))
                    }
                    src = [col[0] * a, col[1] * a, col[2] * a, a]
                }
                out[y * W + x] = over(src, out[y * W + x])
            }
        }
    }
    return out
}

// ---------------------------------------------------------------- the GPU
func render(_ recs: [Rec], _ u: [Float], pipe: MTLRenderPipelineState, W: Int, H: Int, bg: UInt32, repeatDraw: Int = 1) -> ([UInt32], Double) {
    let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: W, height: H, mipmapped: false)
    d.usage = [.renderTarget, .shaderRead]; d.storageMode = .shared
    let tex = dev.makeTexture(descriptor: d)!
    let rp = MTLRenderPassDescriptor()
    rp.colorAttachments[0].texture = tex
    rp.colorAttachments[0].loadAction = .clear
    let bc = unitColor(bg)
    rp.colorAttachments[0].clearColor = MTLClearColor(red: Double(bc[0] * bc[3]), green: Double(bc[1] * bc[3]), blue: Double(bc[2] * bc[3]), alpha: Double(bc[3]))
    rp.colorAttachments[0].storeAction = .store
    let cb = queue.makeCommandBuffer()!
    let e = cb.makeRenderCommandEncoder(descriptor: rp)!
    e.setRenderPipelineState(pipe)
    let rb = recBytes(recs)
    let ib = dev.makeBuffer(bytes: rb, length: max(rb.count, 16), options: .storageModeShared)!
    var gu: [Float] = [Float(W), Float(H), CW, CH, 200, 50, 1024, 1024, 64, 64, 0, 0]
    e.setVertexBuffer(ib, offset: 0, index: 0)
    e.setVertexBytes(&gu, length: gu.count * 4, index: 1)
    var uu = u
    e.setFragmentBytes(&uu, length: uu.count * 4, index: 0)
    for _ in 0 ..< repeatDraw where !recs.isEmpty {
        e.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4, instanceCount: recs.count)
    }
    e.endEncoding(); cb.commit(); cb.waitUntilCompleted()
    var px = [UInt32](repeating: 0, count: W * H)
    tex.getBytes(&px, bytesPerRow: W * 4, from: MTLRegionMake2D(0, 0, W, H), mipmapLevel: 0)
    return (px, cb.gpuEndTime - cb.gpuStartTime)
}

func png(_ px: [UInt32], W: Int, H: Int, _ path: String) {
    var bytes = [UInt8](); bytes.reserveCapacity(W * H * 4)
    for v in px { bytes += [UInt8((v >> 16) & 0xff), UInt8((v >> 8) & 0xff), UInt8(v & 0xff), UInt8(v >> 24)] }
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: &bytes, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W * 4, space: cs,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    let img = ctx.makeImage()!
    let dst = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dst, img, nil); CGImageDestinationFinalize(dst)
}

// ---------------------------------------------------------------- scenes
let W = 1088, H = 544
let scripts: [(String, Script)] = [
    ("across", Script(from: cell(10, 5), to: cell(40, 5), retarget: nil)),
    ("diag", Script(from: cell(5, 1), to: cell(55, 12), retarget: nil)),
    ("retarget", Script(from: cell(10, 5), to: cell(40, 5), retarget: (0.05, cell(25, 13)))),
    ("bar", Script(from: bar(10, 5), to: bar(40, 5), retarget: nil)),
]
let ages: [Float] = [0.0, 0.02, 0.05, 0.08, 0.11, 0.15, 0.2, 0.26, 0.32]
let themes: [(String, UInt32, UInt32)] = [("dark", 0xFF10_1010, 0xD0D0D0), ("light", 0xFFEF_EFEF, 0x272727), ("glass", 0x8010_1010, 0xD0D0D0)]
var worst = 0, stepOff = 0, pastAlpha = 0, lit = 0
var worstAt = ""
for (name, s) in scripts {
    for (tn, bg, fill) in themes {
        var strip = [UInt32]()
        let (plain, _) = render([], uniform(frame(s, 0)), pipe: orbPipe, W: W, H: H, bg: bg)
        for a in ages {
            let f = frame(s, a)
            let recs = records(f, fill: fill, frame: (Float(W), Float(H)))
            let (gpu, _) = render(recs, uniform(f), pipe: orbPipe, W: W, H: H, bg: bg)
            let want = reference(f, recs, plain: plain, W: W)
            for i in 0 ..< gpu.count {
                let x = gpu[i], y = want[i]
                var dmax = 0
                for k in 0 ..< 4 { dmax = max(dmax, abs(Int((x >> (8 * k)) & 0xff) - Int((y >> (8 * k)) & 0xff))) }
                if dmax > worst { worst = dmax; worstAt = "\(name) \(tn) age \(a) px \(i % W),\(i / W): \(String(x, radix: 16)) vs \(String(y, radix: 16))" }
                if dmax == 1 { stepOff += 1 }
                if x != plain[i] { lit += 1 }
                let al = x >> 24
                for k in 0 ..< 3 where (x >> (8 * k)) & 0xff > al { pastAlpha += 1 }
            }
            strip += gpu
        }
        if tn != "glass" { png(strip, W: W, H: H * ages.count, "\(here)/../png/\(name)-\(tn).png") }
    }
}
print("math: \(safe ? "safe/precise" : "fast")")
print("GPU vs CPU reference: worst channel diff \(worst) (\(worstAt)); pixels one step off \(stepOff); lit \(lit); colour past alpha \(pastAlpha)")

// ---------------------------------------------------------------- the reach: the field just inside the cut, uncut outside it
var edgeMax: Float = 0, outMax: Float = 0, edgeAt = "", outAt = ""
for (name, s) in scripts {
    for a in stride(from: Float(0), through: 0.4, by: 0.01) {
        let f = frame(s, a)
        for y in stride(from: 0, to: H, by: 1) {
            for x in stride(from: 0, to: W, by: 1) {
                let p = Pt(x: Float(x) + 0.5, y: Float(y) + 0.5)
                if f.hole.contains(p) { continue }
                let d = quadDistance(p, f.corners)
                let c = REACH * CH
                if d > c - 1.5 && d <= c {
                    let al = 255 * (1 - exp(-density(p, f)))
                    if al > edgeMax { edgeMax = al; edgeAt = "\(name) age \(a) (\(x),\(y))" }
                } else if d > c && d <= c + 40 {
                    let al = 255 * (1 - exp(-density(p, f, cut: false)))
                    if al > outMax { outMax = al; outAt = "\(name) age \(a) (\(x),\(y))" }
                }
            }
        }
    }
}
print(String(format: "reach %.1f cells: field just inside the cut max %.3f of 255 (%@); uncut field past it max %.3f of 255 (%@)", REACH, edgeMax, edgeAt, outMax, outAt))

// ---------------------------------------------------------------- continuity at the retarget
let s3 = scripts[2].1
let e: Float = 1e-4
let fa = frame(s3, 0.05 - e), fb = frame(s3, 0.05 + e)
print(String(format: "retarget at 0.05 s: lead jumps %.3f px, glow %.4f -> %.4f, corners jump %.3f px",
             len(fa.lead - fb.lead), fa.glow, fb.glow, (0 ..< 4).map { len(fa.corners[$0] - fb.corners[$0]) }.max()!))
for (name, s) in scripts {
    let l0 = Lead(from: s.from.center, to: s.to.center)
    var land = l0.lasts
    if let (tr, to2) = s.retarget { land = tr + Lead(from: l0.at(tr), to: to2.center).lasts }
    print(String(format: "%@: lands at %.3f s, end %.3f s (%.0f frames at 120 Hz)", name, land, land + max(ABSORB, PULSE), (land + max(ABSORB, PULSE)) * 120))
}

// ---------------------------------------------------------------- cost: orb against fire over identical bands
let f = frame(scripts[1].1, 0.05)
let bands = records(f, fill: 0xD0D0D0, frame: (Float(W), Float(H)), withMotes: false)
let area = bands.reduce(0) { $0 + Int($1.w) * Int($1.h) }
for round in 0 ..< 5 {
    var to = [Double](), tf = [Double]()
    for _ in 0 ..< 20 {
        to.append(render(bands, uniform(f), pipe: orbPipe, W: W, H: H, bg: 0xFF10_1010, repeatDraw: 20).1)
        tf.append(render(bands, uniform(f, fire: true), pipe: firePipe, W: W, H: H, bg: 0xFF10_1010, repeatDraw: 20).1)
    }
    to.sort(); tf.sort()
    print(String(format: "round %d: %d band px; per draw, median of 20: orb %.1f us, fire %.1f us (ratio %.2f)", round, area, to[10] / 20 * 1e6, tf[10] / 20 * 1e6, tf[10] / to[10]))
}
