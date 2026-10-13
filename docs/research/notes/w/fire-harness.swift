// Measurement harness for a flame cursor trail on Metal. Scratch only; nothing in baud is touched.
// Build: DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -O harness.swift -o harness
import CoreGraphics
import Foundation
import ImageIO
import Metal
import UniformTypeIdentifiers

let dev = MTLCreateSystemDefaultDevice()!
let queue = dev.makeCommandQueue()!
let W = 3456, H = 2234                // this Mac's panel, device px
let cellW: Float = 17, cellH: Float = 34
let gcols = 200, grows = 50
let here = URL(fileURLWithPath: CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : ".")
let mode = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "all"
let bb = try! String(contentsOf: here.appendingPathComponent("bbtable.metal"), encoding: .utf8)
let body = try! String(contentsOf: here.appendingPathComponent("fire_body.metal"), encoding: .utf8)

func source(oct: Int, warp: Bool, effects: Bool) -> String {
    "#include <metal_stdlib>\nusing namespace metal;\n#define OCT \(oct)\n#define WARP \(warp ? 1 : 0)\n#define EFFECTS \(effects ? 1 : 0)\n" + bb + "\n" + body
}
func library(_ s: String, safe: Bool = false) -> MTLLibrary {
    let o = MTLCompileOptions()
    if safe { o.mathMode = .safe; o.mathFloatingPointFunctions = .precise }
    do { return try dev.makeLibrary(source: s, options: o) } catch { fatalError("\(error)") }
}
func pipeline(_ l: MTLLibrary, _ frag: String) -> MTLRenderPipelineState {
    let d = MTLRenderPipelineDescriptor()
    d.vertexFunction = l.makeFunction(name: "glyph_vertex")
    d.fragmentFunction = l.makeFunction(name: frag)
    let a = d.colorAttachments[0]!
    a.pixelFormat = .bgra8Unorm
    a.isBlendingEnabled = true
    a.rgbBlendOperation = .add; a.alphaBlendOperation = .add
    a.sourceRGBBlendFactor = .one; a.sourceAlphaBlendFactor = .one
    a.destinationRGBBlendFactor = .oneMinusSourceAlpha; a.destinationAlphaBlendFactor = .oneMinusSourceAlpha
    return try! dev.makeRenderPipelineState(descriptor: d)
}

// ---- records, byte for byte baud's 20-byte Instance
struct Rec { var x: Int16; var y: Int16; var w: UInt16; var h: UInt16; var u: UInt16; var v: UInt16; var color: UInt32; var kind: UInt32 }
func bytes(_ rs: [Rec]) -> [UInt8] {
    var out = [UInt8](); out.reserveCapacity(rs.count * 20)
    for r in rs {
        for v in [UInt16(bitPattern: r.x), UInt16(bitPattern: r.y), r.w, r.h, r.u, r.v] { out += [UInt8(v & 0xff), UInt8(v >> 8)] }
        for v in [r.color, r.kind] { out += [UInt8(v & 0xff), UInt8((v >> 8) & 0xff), UInt8((v >> 16) & 0xff), UInt8(v >> 24)] }
    }
    return out
}
func buffer(_ b: [UInt8]) -> MTLBuffer { dev.makeBuffer(bytes: b, length: max(b.count, 16), options: .storageModeShared)! }

// PCG, same function the shader uses
@inline(__always) func pcg(_ v: UInt32) -> UInt32 {
    let s = v &* 747796405 &+ 2891336453
    let w = ((s >> ((s >> 28) &+ 4)) ^ s) &* 277803737
    return (w >> 22) ^ w
}
@inline(__always) func u01(_ h: UInt32) -> Float { Float(bitPattern: (h >> 9) | 0x3f80_0000) - 1 }

// glyph grid: 10,000 mask glyphs sampling a random 1024^2 R8 atlas
let maskTex: MTLTexture = {
    let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .r8Unorm, width: 1024, height: 1024, mipmapped: false)
    let t = dev.makeTexture(descriptor: d)!
    var px = [UInt8](repeating: 0, count: 1024 * 1024)
    var st: UInt32 = 1
    for i in 0 ..< px.count { st = pcg(st &+ UInt32(i)); px[i] = (st & 0xff) > 150 ? 255 : 0 }
    t.replace(region: MTLRegionMake2D(0, 0, 1024, 1024), mipmapLevel: 0, withBytes: px, bytesPerRow: 1024)
    return t
}()
let colorTex: MTLTexture = {
    let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm, width: 64, height: 64, mipmapped: false)
    return dev.makeTexture(descriptor: d)!
}()
let sampler: MTLSamplerState = { let d = MTLSamplerDescriptor(); d.minFilter = .nearest; d.magFilter = .nearest; return dev.makeSamplerState(descriptor: d)! }()
let glyphRecs: [Rec] = (0 ..< gcols * grows).map { i in
    let c = i % gcols, r = i / gcols
    let h = pcg(UInt32(i))
    return Rec(x: Int16(c * 17), y: Int16(r * 34), w: 17, h: 34, u: UInt16((h % 60) * 17), v: UInt16(((h >> 8) % 30) * 34),
               color: 0xFFD0D0D0, kind: 0)
}
let glyphBuf = buffer(bytes(glyphRecs))
var U: [Float] = [Float(W), Float(H), cellW, cellH, Float(gcols), Float(grows), 1024, 1024, 64, 64, 0, 0]

// ---- emitter geometry: kitty's four-corner quad in closed form (prior report s.1)
typealias V2 = SIMD2<Float>
struct Move { var fromRC: (Int, Int); var toRC: (Int, Int) }
func rect(_ rc: (Int, Int)) -> (V2, V2) { let x = Float(rc.1) * cellW, y = Float(rc.0) * cellH; return (V2(x, y), V2(x + cellW, y + cellH)) }
func corners(_ a: V2, _ b: V2) -> [V2] { [V2(b.x, a.y), V2(b.x, b.y), V2(a.x, b.y), V2(a.x, a.y)] } // TR BR BL TL
func quad(_ m: Move, t: Float) -> [V2] {
    let (pa, pb) = rect(m.fromRC), (ta, tb) = rect(m.toRC)
    let P = corners(pa, pb), T = corners(ta, tb)
    let cen = (ta + tb) * 0.5
    var dots = [Float]()
    for i in 0 ..< 4 {
        let trav = T[i] - P[i], out = T[i] - cen
        let lt = (trav * trav).sum().squareRoot(), lo = (out * out).sum().squareRoot()
        dots.append(lt < 1e-6 ? 0 : (trav * out).sum() / (lt * lo))
    }
    let mn = dots.min()!, mx = dots.max()!
    return (0 ..< 4).map { i in
        let tau: Float = mx - mn < 1e-6 ? 0.1 : 0.4 + (0.1 - 0.4) * (dots[i] - mn) / (mx - mn)
        return T[i] + (P[i] - T[i]) * pow(2, -10 * t / tau)
    }
}
func fireU(_ m: Move, age: Float, life: Float, seed: UInt32, gain: Float = 1) -> [Float] {
    let q = quad(m, t: age)
    let (pa, pb) = rect(m.fromRC), (ta, tb) = rect(m.toRC)
    let tail = (pa + pb) * 0.5, head = (ta + tb) * 0.5
    let dv = head - tail; let len = max((dv * dv).sum().squareRoot(), 1)
    let dir = dv / len
    var f: [Float] = []
    for c in q { f += [c.x, c.y] }
    f += [ta.x, ta.y, tb.x, tb.y]
    f += [dir.x, dir.y, tail.x, tail.y, len, cellH, age, life, gain, Float(bitPattern: seed), 0, 0]
    return f // 24 floats = 96 bytes, the MSL struct's size
}
let R: Float = (1.6 + 0.4) * cellH  // support radius: FLAME_H + 0.4 cells
// conservative cover: one axis-aligned record per band of rows, clipped to the dilated quad
func bands(_ q: [V2], bandH: Float) -> [Rec] {
    let ys = q.map { $0.y }
    let y0 = max(0, ys.min()! - R), y1 = min(Float(H), ys.max()! + R)
    var out = [Rec]()
    var yb = (y0 / bandH).rounded(.down) * bandH
    while yb < y1 {
        let lo = yb - R, hi = yb + bandH + R
        var xmin = Float.infinity, xmax = -Float.infinity
        for i in 0 ..< 4 {
            let a = q[i], b = q[(i + 1) % 4]
            var t0: Float = 0, t1: Float = 1
            let dy = b.y - a.y
            if abs(dy) < 1e-6 { if a.y < lo || a.y > hi { continue } } else {
                var ta = (lo - a.y) / dy, tb = (hi - a.y) / dy; if ta > tb { swap(&ta, &tb) }
                t0 = max(t0, ta); t1 = min(t1, tb); if t0 > t1 { continue }
            }
            for t in [t0, t1] { let x = a.x + (b.x - a.x) * t; xmin = min(xmin, x); xmax = max(xmax, x) }
        }
        if xmin <= xmax {
            let x0 = max(0, xmin - R), x1 = min(Float(W), xmax + R)
            let yy0 = max(0, yb), yy1 = min(Float(H), yb + bandH)
            if x1 > x0 && yy1 > yy0 {
                out.append(Rec(x: Int16(x0.rounded(.down)), y: Int16(yy0), w: UInt16((x1 - x0).rounded(.up)), h: UInt16(yy1 - yy0), u: 0, v: 0, color: 0, kind: 3))
            }
        }
        yb += bandH
    }
    return out
}
func aabb(_ q: [V2]) -> [Rec] {
    let xs = q.map { $0.x }, ys = q.map { $0.y }
    let x0 = max(0, xs.min()! - R), x1 = min(Float(W), xs.max()! + R), y0 = max(0, ys.min()! - R), y1 = min(Float(H), ys.max()! + R)
    return [Rec(x: Int16(x0), y: Int16(y0), w: UInt16(x1 - x0), h: UInt16(y1 - y0), u: 0, v: 0, color: 0, kind: 3)]
}
func area(_ rs: [Rec]) -> Int { rs.reduce(0) { $0 + Int($1.w) * Int($1.h) } }

// ---- embers: closed-form trajectories evaluated on the CPU per frame, drawn as kind-4 records
let BBtab: [SIMD3<Float>] = {
    // parse the same table the shader uses
    let nums = bb.split(whereSeparator: { !"0123456789.".contains($0) }).compactMap { Float($0) }.filter { _ in true }
    let vals = Array(nums.drop(while: { $0 != 1.0 }))  // skip the "16"
    return stride(from: 0, to: 48, by: 3).map { SIMD3(vals[$0], vals[$0 + 1], vals[$0 + 2]) }
}()
func ramp(_ h: Float) -> SIMD3<Float> {
    let x = max(0, min(1, h)) * 15; let i = min(Int(x), 14); let f = x - Float(i)
    return BBtab[i] + (BBtab[i + 1] - BBtab[i]) * f
}
func embers(_ m: Move, age: Float, n: Int, seed: UInt32) -> [Rec] {
    let (pa, pb) = rect(m.fromRC), (ta, tb) = rect(m.toRC)
    let tail = (pa + pb) * 0.5, head = (ta + tb) * 0.5
    let dv = head - tail; let len = max((dv * dv).sum().squareRoot(), 1); let dir = dv / len
    let perp = V2(-dir.y, dir.x)
    let sweep: Float = 0.10, k: Float = 3.0
    let g = V2(0, -9.0 * cellH)                        // buoyancy, cells/s^2 upward
    var out = [Rec](); out.reserveCapacity(n)
    for i in 0 ..< n {
        let h0 = pcg(seed ^ UInt32(i) &* 0x9E37_79B9), h1 = pcg(h0), h2 = pcg(h1), h3 = pcg(h2), h4 = pcg(h3)
        let e = u01(h0)                                   // where along the path it is born
        let born = e * sweep                              // the emitter reaches it
        let life = 0.30 + 0.45 * u01(h1)
        let a = age - born
        if a < 0 || a > life { continue }
        let p0 = tail + dv * e + perp * (u01(h2) - 0.5) * cellH * 0.8
        let v0 = V2((u01(h3) - 0.5) * 3.0 * cellH, -(1.5 + 3.0 * u01(h4)) * cellH) + dir * min(len / sweep, 40 * cellH) * 0.08
        let ek = exp(-k * a), s = (1 - ek) / k
        let p = p0 + v0 * s + g / k * (a - s)
        let heat = 1 - a / life
        let c = ramp(0.35 + 0.65 * heat) * (0.4 + 0.6 * heat)
        let r = cellH * (0.10 + 0.10 * u01(h2)) * (0.5 + 0.5 * heat)
        let col = (UInt32(255) << 24) | (UInt32(c.x * 255) << 16) | (UInt32(c.y * 255) << 8) | UInt32(c.z * 255)
        out.append(Rec(x: Int16(p.x - r), y: Int16(p.y - r), w: UInt16(max(2, 2 * r)), h: UInt16(max(2, 2 * r)), u: 0, v: 0, color: col, kind: 4))
    }
    return out
}

// ---- libraries and pipelines
let libBase = library(source(oct: 4, warp: false, effects: false))
let pBase = pipeline(libBase, "glyph_fragment")
var libs: [String: MTLLibrary] = [:]
func eff(_ oct: Int, _ warp: Bool, safe: Bool = false) -> MTLLibrary {
    let key = "\(oct)\(warp)\(safe)"
    if let l = libs[key] { return l }
    let l = library(source(oct: oct, warp: warp, effects: true), safe: safe); libs[key] = l; return l
}
let pEff4 = pipeline(eff(4, false), "glyph_fragment")
let pEff3 = pipeline(eff(3, false), "glyph_fragment")
let pEff5 = pipeline(eff(5, false), "glyph_fragment")
let pEff4W = pipeline(eff(4, true), "glyph_fragment")
let pEff4Safe = pipeline(eff(4, false, safe: true), "glyph_fragment")
let pSep = pipeline(eff(4, false), "fire_fragment")

func target(shared: Bool) -> MTLTexture {
    let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: W, height: H, mipmapped: false)
    d.usage = [.renderTarget, .shaderRead]; d.storageMode = shared ? .shared : .private
    return dev.makeTexture(descriptor: d)!
}

struct Scene {
    var glyphs = true
    var pipe: MTLRenderPipelineState
    var firePipe: MTLRenderPipelineState? = nil     // separate pipeline for kind 3
    var move: Move? = nil
    var geom = "bands"                              // bands | aabb
    var bandH: Float = 34
    var embersN = 0
    var fire = true
    var life: Float = 0.45
    var seed: UInt32 = 0x1234_5678
}
var lastCPU: (bands: Double, embers: Double, recs: Int, area: Int) = (0, 0, 0, 0)
func encode(_ enc: MTLRenderCommandEncoder, _ s: Scene, age: Float, frameBuf: inout MTLBuffer?) {
    enc.setRenderPipelineState(s.pipe)
    enc.setVertexBytes(U, length: U.count * 4, index: 1)
    enc.setFragmentTexture(maskTex, index: 0); enc.setFragmentTexture(colorTex, index: 1)
    enc.setFragmentSamplerState(sampler, index: 0)
    if s.glyphs {
        enc.setVertexBuffer(glyphBuf, offset: 0, index: 0)
        enc.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4, instanceCount: glyphRecs.count)
    }
    guard let m = s.move else { return }
    var recs = [Rec]()
    let t0 = DispatchTime.now().uptimeNanoseconds
    if s.fire { let q = quad(m, t: age); recs += s.geom == "bands" ? bands(q, bandH: s.bandH) : aabb(q) }
    let t1 = DispatchTime.now().uptimeNanoseconds
    if s.embersN > 0 { recs += embers(m, age: age, n: s.embersN, seed: s.seed) }
    let t2 = DispatchTime.now().uptimeNanoseconds
    lastCPU = (Double(t1 - t0) / 1e3, Double(t2 - t1) / 1e3, recs.count, area(recs.filter { $0.kind == 3 }))
    if recs.isEmpty { return }
    let fu = fireU(m, age: age, life: s.life, seed: s.seed)
    let b = buffer(bytes(recs)); frameBuf = b
    if let fp = s.firePipe {
        let fire = recs.filter { $0.kind == 3 }
        let fb = buffer(bytes(fire)); frameBuf = fb
        enc.setRenderPipelineState(fp)
        enc.setVertexBuffer(fb, offset: 0, index: 0)
        enc.setFragmentBytes(fu, length: fu.count * 4, index: 0)
        enc.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4, instanceCount: fire.count)
        return
    }
    enc.setVertexBuffer(b, offset: 0, index: 0)
    enc.setFragmentBytes(fu, length: fu.count * 4, index: 0)
    enc.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4, instanceCount: recs.count)
}
func renderOnce(_ s: Scene, age: Float, into t: MTLTexture, clear: MTLClearColor) {
    let cb = queue.makeCommandBuffer()!
    let rp = MTLRenderPassDescriptor()
    rp.colorAttachments[0].texture = t; rp.colorAttachments[0].loadAction = .clear; rp.colorAttachments[0].storeAction = .store
    rp.colorAttachments[0].clearColor = clear
    let enc = cb.makeRenderCommandEncoder(descriptor: rp)!
    var keep: MTLBuffer? = nil
    encode(enc, s, age: age, frameBuf: &keep)
    enc.endEncoding(); cb.commit(); cb.waitUntilCompleted()
}
func readback(_ t: MTLTexture) -> [UInt8] {
    var px = [UInt8](repeating: 0, count: W * H * 4)
    t.getBytes(&px, bytesPerRow: W * 4, from: MTLRegionMake2D(0, 0, W, H), mipmapLevel: 0)
    return px
}
func diff(_ a: [UInt8], _ b: [UInt8]) -> (pixels: Int, maxd: Int) {
    var n = 0, m = 0
    var i = 0
    while i < a.count {
        var d = 0
        for k in 0 ..< 4 { d = max(d, abs(Int(a[i + k]) - Int(b[i + k]))) }
        if d > 0 { n += 1; m = max(m, d) }
        i += 4
    }
    return (n, m)
}
func png(_ px: [UInt8], crop: (Int, Int, Int, Int), to path: String) {
    let (x0, y0, w, h) = crop
    var out = [UInt8](repeating: 0, count: w * h * 4)
    for y in 0 ..< h { for x in 0 ..< w {
        let si = ((y0 + y) * W + (x0 + x)) * 4, di = (y * w + x) * 4
        out[di] = px[si + 2]; out[di + 1] = px[si + 1]; out[di + 2] = px[si]; out[di + 3] = 255
    } }
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: &out, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: cs,
                        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    let img = ctx.makeImage()!
    let dst = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dst, img, nil); CGImageDestinationFinalize(dst)
}

// ---- timing: F frames back to back per config, configs interleaved per round, round 0 discarded
func time(_ s: Scene, frames: Int, t: MTLTexture) -> (gpu: [Double], cpuBands: Double, cpuEmbers: Double, recs: Int, area: Int) {
    var cbs = [MTLCommandBuffer](); var keep = [MTLBuffer?]()
    var cb_ = 0.0, ce = 0.0, recs = 0, ar = 0
    for f in 0 ..< frames {
        let age = Float(f % 54) / 120.0            // a 0.45 s life at 120 Hz, over and over
        let cb = queue.makeCommandBuffer()!
        let rp = MTLRenderPassDescriptor()
        rp.colorAttachments[0].texture = t; rp.colorAttachments[0].loadAction = .clear; rp.colorAttachments[0].storeAction = .store
        rp.colorAttachments[0].clearColor = MTLClearColor(red: 0.06, green: 0.06, blue: 0.06, alpha: 1)
        let enc = cb.makeRenderCommandEncoder(descriptor: rp)!
        var fb: MTLBuffer? = nil
        encode(enc, s, age: age, frameBuf: &fb)
        cb_ += lastCPU.bands; ce += lastCPU.embers; recs = max(recs, lastCPU.recs); ar = max(ar, lastCPU.area)
        keep.append(fb)
        enc.endEncoding(); cb.commit(); cbs.append(cb)
    }
    cbs.last!.waitUntilCompleted()
    for c in cbs { c.waitUntilCompleted() }
    return (cbs.map { ($0.gpuEndTime - $0.gpuStartTime) * 1e6 }, cb_ / Double(frames), ce / Double(frames), recs, ar)
}
func median(_ x: [Double]) -> Double { let s = x.sorted(); return s[s.count / 2] }

let hop5 = Move(fromRC: (20, 40), toRC: (20, 45))
let hop30 = Move(fromRC: (20, 40), toRC: (20, 70))
let hop90 = Move(fromRC: (20, 20), toRC: (20, 110))
let diag = Move(fromRC: (2, 2), toRC: (48, 197))
let vert = Move(fromRC: (5, 100), toRC: (45, 100))

if mode == "all" || mode == "time" {
    var configs: [(String, Scene)] = [
        ("glyphs/base", Scene(pipe: pBase)),
        ("glyphs/base-AA", Scene(pipe: pBase)),
        ("glyphs/effects-compiled", Scene(pipe: pEff4)),
        ("glyphs/effects-safe", Scene(pipe: pEff4Safe)),
    ]
    for (n, m) in [("hop5", hop5), ("hop30", hop30), ("hop90", hop90), ("diag", diag), ("vert40", vert)] {
        configs.append(("fire4/bands/\(n)", Scene(pipe: pEff4, move: m)))
        configs.append(("fire4/aabb/\(n)", Scene(pipe: pEff4, move: m, geom: "aabb")))
    }
    configs += [
        ("fire3/bands/hop30", Scene(pipe: pEff3, move: hop30)),
        ("fire5/bands/hop30", Scene(pipe: pEff5, move: hop30)),
        ("fire4warp/bands/hop30", Scene(pipe: pEff4W, move: hop30)),
        ("fire3/bands/diag", Scene(pipe: pEff3, move: diag)),
        ("fire5/bands/diag", Scene(pipe: pEff5, move: diag)),
        ("fire4warp/bands/diag", Scene(pipe: pEff4W, move: diag)),
        ("fire4safe/bands/hop30", Scene(pipe: pEff4Safe, move: hop30)),
        ("fire4-separate-pipe/bands/hop30", Scene(pipe: pBase, firePipe: pSep, move: hop30)),
        ("embers64/hop30", Scene(pipe: pEff4, move: hop30, embersN: 64, fire: false)),
        ("embers256/hop30", Scene(pipe: pEff4, move: hop30, embersN: 256, fire: false)),
        ("embers1024/hop30", Scene(pipe: pEff4, move: hop30, embersN: 1024, fire: false)),
        ("fire4+embers128/bands/hop30", Scene(pipe: pEff4, move: hop30, embersN: 128)),
        ("fire4+embers128/bands/diag", Scene(pipe: pEff4, move: diag, embersN: 128)),
        ("fire-only-noglyphs/bands/hop30", Scene(glyphs: false, pipe: pEff4, move: hop30)),
        ("noglyphs-nofire", Scene(glyphs: false, pipe: pEff4)),
    ]
    let rounds = 7, frames = 108
    let t = target(shared: false)
    var res: [String: [Double]] = [:]
    var meta: [String: (Double, Double, Int, Int)] = [:]
    for r in 0 ..< rounds {
        let order = configs.indices.map { (($0 + r * 5) % configs.count) }
        for i in order {
            let (name, s) = configs[i]
            let out = time(s, frames: frames, t: t)
            if r == 0 { continue }
            res[name, default: []].append(median(out.gpu))
            meta[name] = (out.cpuBands, out.cpuEmbers, out.recs, out.area)
        }
    }
    print("config | GPU us/frame median-of-round-medians [min-max over \(rounds - 1) rounds] | CPU us bands, embers | max records | max fire px")
    for (name, _) in configs {
        let v = res[name]!
        let m = meta[name]!
        print(String(format: "%-34@ %8.1f [%7.1f-%7.1f]  cpu %6.1f %6.1f  recs %4d  px %8d", name as NSString, median(v), v.min()!, v.max()!, m.0, m.1, m.2, m.3))
    }
}

if mode == "all" || mode == "det" {
    let a = target(shared: true), b = target(shared: true)
    let clear = MTLClearColor(red: 0.06, green: 0.06, blue: 0.06, alpha: 1)
    // D1: same PSO, two command buffers, a heavy unrelated frame between
    renderOnce(Scene(pipe: pEff4, move: hop30), age: 0.12, into: a, clear: clear)
    renderOnce(Scene(pipe: pEff5, move: diag, embersN: 256), age: 0.3, into: b, clear: clear)
    renderOnce(Scene(pipe: pEff4, move: hop30), age: 0.12, into: b, clear: clear)
    let pa = readback(a), pb = readback(b)
    print("D1 same PSO, separate command buffers:", diff(pa, pb))
    // D2: same fire() body in its own pipeline vs inside glyph_fragment (glyphs drawn by base pipe first)
    renderOnce(Scene(pipe: pBase, firePipe: pSep, move: hop30), age: 0.12, into: b, clear: clear)
    print("D2 fire in glyph_fragment vs separate fire_fragment PSO:", diff(pa, readback(b)))
    // D3: fast vs safe math
    renderOnce(Scene(pipe: pEff4Safe, move: hop30), age: 0.12, into: b, clear: clear)
    let pSafe = readback(b)
    print("D3 fast vs safe math (same source):", diff(pa, pSafe))
    // D4: AABB vs bands coverage (fire only, both on diag)
    renderOnce(Scene(glyphs: false, pipe: pEff4, move: diag, geom: "aabb"), age: 0.1, into: a, clear: clear)
    renderOnce(Scene(glyphs: false, pipe: pEff4, move: diag, geom: "bands"), age: 0.1, into: b, clear: clear)
    print("D4 diag: one AABB record vs banded records:", diff(readback(a), readback(b)))
    // D5: glyph-only frames through base vs effects-compiled glyph_fragment
    renderOnce(Scene(pipe: pBase), age: 0, into: a, clear: clear)
    renderOnce(Scene(pipe: pEff4), age: 0, into: b, clear: clear)
    print("D5 glyphs only, base vs effects-compiled:", diff(readback(a), readback(b)))
    // D6: fire-only pixel count at a few ages (how much of the frame the effect touches)
    for age in [Float(0.02), 0.1, 0.2, 0.3, 0.4] {
        renderOnce(Scene(glyphs: false, pipe: pEff4, move: hop30), age: age, into: a, clear: clear)
        renderOnce(Scene(glyphs: false, pipe: pEff4, move: hop30, fire: false), age: age, into: b, clear: clear)
        print(String(format: "D6 hop30 age %.2f: pixels the flame changed", age), diff(readback(a), readback(b)).pixels)
    }
}

if mode == "all" || mode == "png" {
    let t = target(shared: true)
    let dark = MTLClearColor(red: 16 / 255, green: 16 / 255, blue: 16 / 255, alpha: 1)
    let light = MTLClearColor(red: 239 / 255, green: 239 / 255, blue: 239 / 255, alpha: 1)
    let out = here.appendingPathComponent("png").path
    try? FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)
    for age in [Float(0.03), 0.08, 0.15, 0.25, 0.35] {
        renderOnce(Scene(glyphs: false, pipe: pEff4W, move: hop30), age: age, into: t, clear: dark)
        png(readback(t), crop: (560, 520, 900, 300), to: "\(out)/flame_hop30_\(Int(age * 100))cs.png")
        renderOnce(Scene(glyphs: false, pipe: pEff4W, move: hop30, embersN: 128), age: age, into: t, clear: dark)
        png(readback(t), crop: (560, 520, 900, 300), to: "\(out)/hybrid_hop30_\(Int(age * 100))cs.png")
        renderOnce(Scene(glyphs: false, pipe: pEff4, move: hop30, embersN: 160, fire: false), age: age, into: t, clear: dark)
        png(readback(t), crop: (560, 520, 900, 300), to: "\(out)/embers_hop30_\(Int(age * 100))cs.png")
    }
    for age in [Float(0.05), 0.15, 0.3] {
        renderOnce(Scene(glyphs: false, pipe: pEff4W, move: diag, embersN: 128), age: age, into: t, clear: dark)
        png(readback(t), crop: (0, 0, W, H), to: "\(out)/hybrid_diag_\(Int(age * 100))cs.png")
        renderOnce(Scene(glyphs: false, pipe: pEff4W, move: hop30), age: age, into: t, clear: light)
        png(readback(t), crop: (560, 520, 900, 300), to: "\(out)/flame_hop30_light_\(Int(age * 100))cs.png")
    }
    renderOnce(Scene(glyphs: true, pipe: pEff4W, move: hop30, embersN: 128), age: 0.1, into: t, clear: dark)
    png(readback(t), crop: (560, 520, 900, 300), to: "\(out)/hybrid_hop30_over_glyphs.png")
    print("pngs in", out)
}

// ---- photosensitivity check: per-pixel relative luminance over one life at 120 Hz
if mode == "lum" {
    let t = target(shared: true)
    let dark = MTLClearColor(red: 16 / 255, green: 16 / 255, blue: 16 / 255, alpha: 1)
    func lin(_ c: UInt8) -> Float { let v = Float(c) / 255; return v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
    let lut = (0 ..< 256).map { lin(UInt8($0)) }
    for (name, m, crop) in [("hop30", hop30, (560, 520, 900, 300)), ("hop90", hop90, (200, 520, 1900, 300))] {
        let (x0, y0, w, h) = crop
        let frames = 60
        var L = [Float](repeating: 0, count: w * h * frames)
        var red = [Bool](repeating: false, count: w * h)
        var meanL = [Float]()
        for f in 0 ..< frames {
            renderOnce(Scene(glyphs: false, pipe: pEff4, move: m, embersN: 128), age: Float(f) / 120, into: t, clear: dark)
            let px = readback(t)
            var sum: Float = 0
            for y in 0 ..< h { for x in 0 ..< w {
                let si = ((y0 + y) * W + x0 + x) * 4
                let b = lut[Int(px[si])], g = lut[Int(px[si + 1])], r = lut[Int(px[si + 2])]
                let l = 0.2126 * r + 0.7152 * g + 0.0722 * b
                L[(y * w + x) * frames + f] = l; sum += l
                if r + g + b > 0.05 && r / (r + g + b) >= 0.8 { red[y * w + x] = true }
            } }
            meanL.append(sum / Float(w * h))
        }
        // flashes: pairs of opposing transitions of >= 0.1 relative luminance, darker state < 0.8 (WCAG 2.2 definition)
        var hist = [Int](repeating: 0, count: 6)
        for i in 0 ..< w * h {
            var trans = 0, dirUp = 0, ext = L[i * frames]
            for f in 1 ..< frames {
                let v = L[i * frames + f]
                if dirUp >= 0 && v > ext { ext = v; if dirUp == 0 { } }
                if dirUp <= 0 && v < ext { ext = v }
                // hysteresis: register a transition once the swing from the last extreme reaches 0.1
                if dirUp <= 0 && v - ext >= 0.1 && ext < 0.8 { trans += 1; dirUp = 1; ext = v }
                else if dirUp >= 0 && ext - v >= 0.1 && v < 0.8 { trans += 1; dirUp = -1; ext = v }
                if dirUp == 1 && v > ext { ext = v }
                if dirUp == -1 && v < ext { ext = v }
            }
            hist[min(trans / 2, 5)] += 1
        }
        let lit = (0 ..< w * h).filter { i in (0 ..< frames).contains { L[i * frames + $0] > 0.02 } }.count
        print("\(name): crop \(w)x\(h), pixels ever lit \(lit), flash-count histogram (pairs of opposing >=0.1 changes over 0.5 s) 0:\(hist[0]) 1:\(hist[1]) 2:\(hist[2]) 3:\(hist[3]) 4:\(hist[4]) 5+:\(hist[5]); pixels ever saturated-red: \(red.filter { $0 }.count)")
        print("  area-mean relative luminance per frame:", meanL.map { String(format: "%.4f", $0) }.joined(separator: " "))
    }
}
